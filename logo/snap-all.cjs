// 批量导出 16/20/24/32/40/48/256 的 PNG，只截取 .badge 元素区域，保持 HTML/字号不变
const fs = require('fs');
const path = require('path');
const puppeteer = require('puppeteer');
const sharp = require('sharp');

(async () => {
  const sizes = [16, 20, 24, 32, 40, 48, 256];
  const root = __dirname;
  const browser = await puppeteer.launch({ headless: 'new' });
  const page = await browser.newPage();

  for (const size of sizes) {
    const htmlFile = path.join(root, `${size}.html`);
    if (!fs.existsSync(htmlFile)) continue;
    const outPng = path.join(root, `${size}.png`);
    // 小尺寸采用更高的 DPR 渲染，随后使用 Lanczos 下采样，减少字体锯齿与模糊
    const scale = size <= 16 ? 12 : size <= 20 ? 10 : size <= 24 ? 10 : size <= 32 ? 8 : size <= 40 ? 7 : size <= 48 ? 6 : 1;
    await page.setViewport({ width: size, height: size, deviceScaleFactor: scale });
    await page.goto('file://' + htmlFile.replace(/\\/g, '/'));
    await page.waitForSelector('.badge', { timeout: 5000 });
    const el = await page.$('.badge');
    if (!el) throw new Error(`未找到 .badge: ${size}.html`);
    const rawBuf = await el.screenshot({ type: 'png' });
    const base = sharp(rawBuf).removeAlpha();
    const resized = scale > 1
      ? base.resize(size, size, { fit: 'fill', kernel: sharp.kernel.lanczos3 })
      : base;
    // 轻微锐化，增强边缘对比而不过度产生光晕
    await resized.sharpen({ sigma: 0.6, m1: 0.6, m2: 0.2, x1: 3 })
      .png({ compressionLevel: 9, adaptiveFiltering: true })
      .toFile(outPng);
    console.log('Saved:', outPng, `(scale x${scale})`);
  }

  await browser.close();
})();


