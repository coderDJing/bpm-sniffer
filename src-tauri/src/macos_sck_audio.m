#import <Foundation/Foundation.h>
#import <ScreenCaptureKit/ScreenCaptureKit.h>
#import <CoreMedia/CoreMedia.h>
#import <AudioToolbox/AudioFormat.h>
#import <CoreAudio/CoreAudio.h>

// 将 crossbeam_channel::Sender<Vec<f32>> 的裸指针传入，回调时发送单声道 f32 帧
static SCStream* gStream = nil;
static id gStreamOutput = nil;
static void* gTxPtr = NULL;
static id gOutputChangeObserver = nil;

@interface AudioOnlyOutput : NSObject<SCStreamOutput>
@end

@implementation AudioOnlyOutput
- (void)stream:(SCStream *)stream didOutputAudioSampleBuffer:(CMSampleBufferRef)sampleBuffer ofType:(SCStreamOutputType)type {
  if (type != SCStreamOutputTypeAudio) { return; }
  if (gTxPtr == NULL) { return; }

  // 读取格式描述
  CMAudioFormatDescriptionRef fmt = (CMAudioFormatDescriptionRef)CMSampleBufferGetFormatDescription(sampleBuffer);
  if (!fmt) { return; }
  const AudioStreamBasicDescription *asbd = CMAudioFormatDescriptionGetStreamBasicDescription(fmt);
  if (!asbd) { return; }
  UInt32 channels = asbd->mChannelsPerFrame;
  if (channels == 0) { return; }

  // 读取 AudioBufferList（兼容交错/平面）
  CMBlockBufferRef blockBuf = NULL;
  AudioBufferList abl;
  memset(&abl, 0, sizeof(abl));
  abl.mNumberBuffers = 0;
  OSStatus st = CMSampleBufferGetAudioBufferListWithRetainedBlockBuffer(
    sampleBuffer,
    NULL,
    &abl,
    sizeof(abl),
    NULL,
    NULL,
    kCMSampleBufferFlag_AudioBufferList_Assure16ByteAlignment,
    &blockBuf
  );
  if (st != noErr) { return; }

  // 准备输出 mono
  SInt64 nFrames = CMSampleBufferGetNumSamples(sampleBuffer);
  if (nFrames <= 0) { if (blockBuf) CFRelease(blockBuf); return; }
  NSMutableData *mono = [NSMutableData dataWithLength: (NSUInteger)nFrames * sizeof(float)];
  float *m = (float*)mono.mutableBytes;

  BOOL isFloat = (asbd->mFormatFlags & kAudioFormatFlagIsFloat) != 0;
  BOOL nonInterleaved = (asbd->mFormatFlags & kAudioFormatFlagIsNonInterleaved) != 0;

  if (isFloat) {
    if (nonInterleaved) {
      // 平面：每个 buffer 一个通道
      UInt32 nb = abl.mNumberBuffers;
      if (nb == 0) { if (blockBuf) CFRelease(blockBuf); return; }
      // 累加各通道后取平均
      memset(m, 0, (size_t)nFrames * sizeof(float));
      for (UInt32 b = 0; b < nb; ++b) {
        AudioBuffer buf = abl.mBuffers[b];
        const float *pf = (const float*)buf.mData;
        UInt32 count = (UInt32)(buf.mDataByteSize / sizeof(float));
        UInt32 frames = (UInt32)MIN((SInt64)count, nFrames);
        for (UInt32 i = 0; i < frames; ++i) { m[i] += pf[i]; }
      }
      float inv = 1.0f / (float)nb;
      for (SInt64 i = 0; i < nFrames; ++i) { m[i] *= inv; }
    } else {
      // 交错：单个 buffer，按通道步长混合
      if (abl.mNumberBuffers < 1) { if (blockBuf) CFRelease(blockBuf); return; }
      AudioBuffer buf = abl.mBuffers[0];
      const float *pf = (const float*)buf.mData;
      UInt32 count = (UInt32)(buf.mDataByteSize / sizeof(float));
      UInt32 ch = MAX(channels, 1);
      UInt32 frames = (UInt32)MIN((SInt64)(count / ch), nFrames);
      for (UInt32 i = 0; i < frames; ++i) {
        double acc = 0.0;
        for (UInt32 c = 0; c < ch; ++c) { acc += pf[i*ch + c]; }
        m[i] = (float)(acc / (double)ch);
      }
    }
  } else {
    // 整型路径（常见 int16），转换为 [-1,1]
    if (nonInterleaved) {
      UInt32 nb = abl.mNumberBuffers;
      if (nb == 0) { if (blockBuf) CFRelease(blockBuf); return; }
      memset(m, 0, (size_t)nFrames * sizeof(float));
      for (UInt32 b = 0; b < nb; ++b) {
        AudioBuffer buf = abl.mBuffers[b];
        const SInt16 *ps = (const SInt16*)buf.mData; // 假定 16-bit；其它位宽可后续扩展
        UInt32 count = (UInt32)(buf.mDataByteSize / sizeof(SInt16));
        UInt32 frames = (UInt32)MIN((SInt64)count, nFrames);
        for (UInt32 i = 0; i < frames; ++i) { m[i] += (float)ps[i] / 32768.0f; }
      }
      float inv = 1.0f / (float)nb;
      for (SInt64 i = 0; i < nFrames; ++i) { m[i] *= inv; }
    } else {
      if (abl.mNumberBuffers < 1) { if (blockBuf) CFRelease(blockBuf); return; }
      AudioBuffer buf = abl.mBuffers[0];
      const SInt16 *ps = (const SInt16*)buf.mData;
      UInt32 count = (UInt32)(buf.mDataByteSize / sizeof(SInt16));
      UInt32 ch = MAX(channels, 1);
      UInt32 frames = (UInt32)MIN((SInt64)(count / ch), nFrames);
      for (UInt32 i = 0; i < frames; ++i) {
        double acc = 0.0;
        for (UInt32 c = 0; c < ch; ++c) { acc += (double)ps[i*ch + c] / 32768.0; }
        m[i] = (float)(acc / (double)ch);
      }
    }
  }

  // 回到 Rust：将 mono 发送出去
  typedef struct SenderOpaque SenderOpaque;
  extern bool rs_send_f32_buffer(SenderOpaque* tx, const float* data, size_t len);
  rs_send_f32_buffer((SenderOpaque*)gTxPtr, m, (size_t)nFrames);
  if (blockBuf) CFRelease(blockBuf);
}
@end

bool sck_audio_start(void* tx_ptr) {
  @autoreleasepool {
    gTxPtr = tx_ptr;
    if (@available(macOS 13.0, *)) {
      // 异步获取可共享内容，然后启动仅音频流
      [SCShareableContent getShareableContentWithCompletionHandler:^(SCShareableContent * _Nullable content, NSError * _Nullable error) {
        if (error || content.displays.count == 0) { return; }
        SCDisplay* display = content.displays.firstObject;
        SCContentFilter* filter = [[SCContentFilter alloc] initWithDisplay:display excludingWindows:@[]];
        // macOS 13+：使用 SCStreamConfiguration 捕获系统音频
        SCStreamConfiguration* cfg = [SCStreamConfiguration new];
        cfg.capturesAudio = YES;
        cfg.capturesVideo = NO;
        gStream = [[SCStream alloc] initWithFilter:filter configuration:cfg delegate:nil];
        gStreamOutput = [AudioOnlyOutput new];
        if ([gStream respondsToSelector:@selector(addStreamOutput:type:sampleHandlerQueue:)]) {
          [gStream addStreamOutput:gStreamOutput type:SCStreamOutputTypeAudio sampleHandlerQueue:dispatch_get_global_queue(QOS_CLASS_USER_INTERACTIVE, 0)];
        }
        [gStream startCaptureWithCompletionHandler:^(NSError * _Nullable err2) {
          // 可在此记录错误或成功日志
        }];
      }];
      // 监听默认输出设备变化（通过通知中心）并重建流
      // macOS 上没有 AVAudioSession 事件，这里暂不监听输出路由变化
      return true; // 提前返回，不阻塞
    } else {
      return false;
    }
  }
}

void sck_audio_stop(void) {
  @autoreleasepool {
    if (gStream) {
      [gStream stopCaptureWithCompletionHandler:^(NSError * _Nullable error) {}];
      gStream = nil;
    }
    gStreamOutput = nil;
    if (gOutputChangeObserver) {
      [[NSNotificationCenter defaultCenter] removeObserver:gOutputChangeObserver];
      gOutputChangeObserver = nil;
    }
    gTxPtr = NULL;
  }
}


