#!/usr/bin/env node

/**
 * Generate all platform icons from the source SVG
 *
 * This script reads frontend/app/public/favicon.svg and generates:
 * - PNG icons in various sizes for desktop and mobile
 * - ICO files for Windows
 *
 * Uses Playwright to render SVG in a real browser for pixel-perfect consistency
 * with the web version.
 *
 * Usage: node scripts/generate-icons.js
 */

import { readFile, writeFile, mkdir, rm, cp } from 'node:fs/promises';
import { execFile as execFileCallback } from 'node:child_process';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { promisify } from 'node:util';
import { chromium } from 'playwright';

const __dirname = dirname(fileURLToPath(import.meta.url));
const rootDir = join(__dirname, '..');
const execFile = promisify(execFileCallback);

// Source SVG file
const SOURCE_SVG = join(rootDir, 'frontend/app/public/favicon.svg');

// Target directories
const DESKTOP_ICONS = join(rootDir, 'apps/desktop/src-tauri/icons');
const MOBILE_ICONS = join(rootDir, 'apps/mobile/src-tauri/icons');
const MOBILE_ANDROID_RES = join(
  rootDir,
  'apps/mobile/src-tauri/gen/android/app/src/main/res',
);
const MOBILE_ANDROID_ICON_DIRS = [
  'mipmap-anydpi-v26',
  'mipmap-hdpi',
  'mipmap-mdpi',
  'mipmap-xhdpi',
  'mipmap-xxhdpi',
  'mipmap-xxxhdpi',
  'values',
];
const DESKTOP_PLATFORM_ICON_FILES = ['icon.icns'];

// Icon sizes to generate
const SIZES = [
  { name: '32x32.png', size: 32 },
  { name: '128x128.png', size: 128 },
  { name: '128x128@2x.png', size: 256 },
  { name: 'icon.png', size: 1024 },
];

// Keep the largest frame first so naive consumers do not lock onto a tiny icon.
const ICO_SIZES = [256, 128, 64, 48, 40, 32, 24, 20, 16];
const SMALL_ICON_MAX_SIZE = 40;

/**
 * Adjust tiny sizes so the braces stay legible in the Windows taskbar.
 */
function getSvgContentForSize(svgContent, size) {
  if (size > SMALL_ICON_MAX_SIZE) {
    return svgContent;
  }

  return svgContent
    .replace(/\sfilter="url\(#braceGlow\)"/g, '')
    .replace(/font-size="20"/g, 'font-size="22"')
    .replace(/font-weight="700"/g, 'font-weight="800"');
}

/**
 * Render the SVG at a specific size and preserve the alpha channel in the PNG.
 */
async function renderSvgToPng(page, svgContent, size) {
  const sizedSvgContent = getSvgContentForSize(svgContent, size);

  await page.setViewportSize({
    width: size,
    height: size,
  });

  const html = `
    <!DOCTYPE html>
    <html>
      <head>
        <style>
          * { margin: 0; padding: 0; }
          html, body {
            width: ${size}px;
            height: ${size}px;
            overflow: hidden;
            background: transparent;
          }
          svg {
            width: 100%;
            height: 100%;
            display: block;
          }
        </style>
      </head>
      <body>${sizedSvgContent}</body>
    </html>
  `;

  await page.setContent(html);
  await page.waitForLoadState('networkidle');

  // Wait for fonts and filters to render
  await page.waitForTimeout(100);

  return page.screenshot({
    type: 'png',
    omitBackground: true,
  });
}

/**
 * Generate a PNG from SVG at specified size using a reusable page
 */
async function generatePng(page, svgContent, size, outputPath) {
  console.log(`  Rendering ${size}x${size}...`);
  const screenshot = await renderSvgToPng(page, svgContent, size);

  await writeFile(outputPath, screenshot);
  console.log(`  ✓ Generated ${outputPath} (${size}x${size})`);

  return screenshot;
}

/**
 * Generate ICO file from PNG buffers
 */
async function generateIco(pngBuffers, outputPath) {
  // ICO file format structure
  const icoHeader = Buffer.alloc(6);
  icoHeader.writeUInt16LE(0, 0); // Reserved (must be 0)
  icoHeader.writeUInt16LE(1, 2); // Type (1 = ICO)
  icoHeader.writeUInt16LE(pngBuffers.length, 4); // Number of images

  const iconDirEntries = [];
  const imageDataBuffers = [];
  let imageDataOffset = 6 + (pngBuffers.length * 16); // Header + directory entries

  for (let i = 0; i < pngBuffers.length; i++) {
    const { size, buffer } = pngBuffers[i];

    const entry = Buffer.alloc(16);
    entry.writeUInt8(size === 256 ? 0 : size, 0); // Width (0 means 256)
    entry.writeUInt8(size === 256 ? 0 : size, 1); // Height (0 means 256)
    entry.writeUInt8(0, 2); // Color palette (0 = no palette)
    entry.writeUInt8(0, 3); // Reserved
    entry.writeUInt16LE(1, 4); // Color planes
    entry.writeUInt16LE(32, 6); // Bits per pixel
    entry.writeUInt32LE(buffer.length, 8); // Image data size
    entry.writeUInt32LE(imageDataOffset, 12); // Image data offset

    iconDirEntries.push(entry);
    imageDataBuffers.push(buffer);
    imageDataOffset += buffer.length;
  }

  const icoBuffer = Buffer.concat([
    icoHeader,
    ...iconDirEntries,
    ...imageDataBuffers,
  ]);

  await writeFile(outputPath, icoBuffer);
  console.log(`  ✓ Generated ${outputPath} (${ICO_SIZES.join(', ')}px)`);
}

/**
 * Generate all icons for a target directory
 */
async function generateIconsForTarget(page, svgContent, targetDir) {
  await mkdir(targetDir, { recursive: true });

  // Generate PNG files
  for (const { name, size } of SIZES) {
    const outputPath = join(targetDir, name);
    await generatePng(page, svgContent, size, outputPath);
  }

  // Generate ICO file - render all sizes first
  console.log(`  Rendering ICO sizes...`);
  const icoPngBuffers = [];

  for (const size of ICO_SIZES) {
    const screenshot = await renderSvgToPng(page, svgContent, size);
    icoPngBuffers.push({ size, buffer: screenshot });
  }

  const icoPath = join(targetDir, 'icon.ico');
  await generateIco(icoPngBuffers, icoPath);
}

/**
 * Generate desktop platform icons that are awkward to render directly in JS,
 * while keeping the browser-rendered PNG as the source of truth.
 */
async function generateDesktopPlatformIcons(sourceIconPath, targetDir) {
  const tempDir = join(targetDir, '.tauri-icon-tmp');

  await rm(tempDir, { recursive: true, force: true });
  await mkdir(tempDir, { recursive: true });

  try {
    await execFile('cargo', [
      'tauri',
      'icon',
      sourceIconPath,
      '--output',
      tempDir,
    ], {
      cwd: rootDir,
    });

    for (const fileName of DESKTOP_PLATFORM_ICON_FILES) {
      await cp(join(tempDir, fileName), join(targetDir, fileName));
    }
  } finally {
    await rm(tempDir, { recursive: true, force: true });
  }
}

/**
 * Generate Android and iOS icon resources for the mobile app from the
 * already-rendered 1024x1024 PNG. This keeps the browser-rendered master icon
 * while letting Tauri produce the platform-specific resource trees it expects.
 */
async function generateMobilePlatformIcons(sourceIconPath, targetDir) {
  const tempDir = join(targetDir, '.tauri-icon-tmp');

  await rm(tempDir, { recursive: true, force: true });
  await mkdir(tempDir, { recursive: true });

  try {
    await execFile('cargo', [
      'tauri',
      'icon',
      sourceIconPath,
      '--output',
      tempDir,
    ], {
      cwd: rootDir,
    });

    await rm(join(targetDir, 'android'), { recursive: true, force: true });
    await rm(join(targetDir, 'ios'), { recursive: true, force: true });

    await cp(join(tempDir, 'android'), join(targetDir, 'android'), {
      recursive: true,
    });
    await cp(join(tempDir, 'ios'), join(targetDir, 'ios'), {
      recursive: true,
    });
  } finally {
    await rm(tempDir, { recursive: true, force: true });
  }
}

/**
 * Sync the generated Android launcher assets into the generated mobile project
 * so the checked-in Android project matches the mobile icon set.
 */
async function syncMobileAndroidResources(targetDir) {
  await mkdir(MOBILE_ANDROID_RES, { recursive: true });

  for (const directory of MOBILE_ANDROID_ICON_DIRS) {
    await cp(
      join(targetDir, 'android', directory),
      join(MOBILE_ANDROID_RES, directory),
      { recursive: true },
    );
  }
}

/**
 * Main function
 */
async function main() {
  console.log('🎨 Generating icons from SVG using browser rendering...\n');
  console.log(`Source: ${SOURCE_SVG}\n`);

  // Read source SVG
  const svgContent = await readFile(SOURCE_SVG, 'utf-8');

  // Launch browser once for all operations
  console.log('🌐 Launching browser...');
  const browser = await chromium.launch({
    headless: true,
  });

  try {
    // Create a single page to reuse
    const page = await browser.newPage();

    // Generate icons for desktop
    console.log('\n📱 Desktop icons:');
    await generateIconsForTarget(page, svgContent, DESKTOP_ICONS);
    await generateDesktopPlatformIcons(join(DESKTOP_ICONS, 'icon.png'), DESKTOP_ICONS);

    // Generate icons for mobile
    console.log('\n📱 Mobile icons:');
    await generateIconsForTarget(page, svgContent, MOBILE_ICONS);
    await generateMobilePlatformIcons(join(MOBILE_ICONS, 'icon.png'), MOBILE_ICONS);
    await syncMobileAndroidResources(MOBILE_ICONS);

    await page.close();

    console.log('\n✅ All icons generated successfully!');
    console.log('\nℹ️  Icons are rendered using Chromium for pixel-perfect consistency with web.');
    console.log('\nNext steps:');
    console.log('1. Review the generated icons');
    console.log('2. Test the apps to ensure icons display correctly');
    console.log('3. Commit the changes');
  } finally {
    await browser.close();
  }
}

main().catch((error) => {
  console.error('❌ Error generating icons:', error);
  process.exit(1);
});
