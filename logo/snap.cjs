// 仅截图 .badge 元素为 256x256，不改动 HTML 与字号
const path = require('path');
const puppeteer = require('puppeteer');

(async () => {
  const root = __dirname;
  const file = path.join(root, '256.html');
  const out = path.join(root, '256.png');

  const browser = await puppeteer.launch({ headless: 'new', args: ['--force-device-scale-factor=1'] });
  const page = await browser.newPage();
  await page.setViewport({ width: 256, height: 256, deviceScaleFactor: 1 });
  await page.goto('file://' + file.replace(/\\/g, '/'));
  const handle = await page.$('.badge');
  if (!handle) throw new Error('未找到 .badge 元素');
  await handle.screenshot({ path: out });
  await browser.close();
  console.log('Saved:', out);
})();


