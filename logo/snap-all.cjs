// 批量导出 16/32/48/256 的 PNG，只截取 .badge 元素区域，保持 HTML/字号不变
const fs = require('fs');
const path = require('path');
const puppeteer = require('puppeteer');

(async () => {
  const sizes = [16, 32, 48, 256];
  const root = __dirname;
  const browser = await puppeteer.launch({ headless: 'new', args: ['--force-device-scale-factor=1'] });
  const page = await browser.newPage();

  for (const size of sizes) {
    const htmlFile = path.join(root, `${size}.html`);
    if (!fs.existsSync(htmlFile)) continue;
    const outPng = path.join(root, `${size}.png`);
    await page.setViewport({ width: size, height: size, deviceScaleFactor: 1 });
    await page.goto('file://' + htmlFile.replace(/\\/g, '/'));
    await page.waitForSelector('.badge', { timeout: 5000 });
    const el = await page.$('.badge');
    if (!el) throw new Error(`未找到 .badge: ${size}.html`);
    await el.screenshot({ path: outPng });
    console.log('Saved:', outPng);
  }

  await browser.close();
})();


