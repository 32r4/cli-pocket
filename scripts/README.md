# Icon Generation Script

This directory contains scripts for maintaining the project's icons.

## generate-icons.js

Generates all platform-specific icons from the source SVG file using **browser rendering** for pixel-perfect consistency with the web version.

### How It Works

The script uses **Playwright** to render the SVG in a real Chromium browser, ensuring:
- Identical rendering to what users see in the web app
- Proper font rendering (Consolas/Courier New)
- Accurate SVG filter effects (glow, gradients)
- Consistent text positioning and styling

This approach solves the common issue where server-side SVG renderers (like librsvg/resvg) produce different results than browsers.

### Source

- **Input**: `frontend/app/public/favicon.svg`
- **Outputs**:
  - `apps/desktop/src-tauri/icons/` (all sizes)
  - `apps/mobile/src-tauri/icons/` (all common, Android, and iOS sizes)
  - `apps/mobile/src-tauri/gen/android/app/src/main/res/` (Android launcher resources)

### Generated Files

For each platform (desktop and mobile):

- `icon.png` (1024x1024) - Main icon
- `128x128.png` (128x128) - Medium size
- `128x128@2x.png` (256x256) - Retina display
- `32x32.png` (32x32) - Small size
- `icon.ico` - Windows icon (contains 16, 32, 48, 64, 128, 256px)

For mobile, the script also runs `cargo tauri icon` from the browser-rendered
`icon.png` and updates:

- `apps/mobile/src-tauri/icons/android/`
- `apps/mobile/src-tauri/icons/ios/`
- `apps/mobile/src-tauri/gen/android/app/src/main/res/mipmap-*`
- `apps/mobile/src-tauri/gen/android/app/src/main/res/values`

### Usage

```bash
# Install dependencies (first time only)
npm install
npx playwright install chromium

# Generate all icons
npm run generate-icons
```

### When to Run

Run this script whenever you:
- Update the source SVG (`frontend/app/public/favicon.svg`)
- Add a new platform that needs icons
- Need to regenerate icons for any reason

Mobile builds run this script automatically before the Tauri mobile dev and
build commands, so the generated Android launcher resources stay in sync with
the source SVG.

### Maintenance

To modify the icon design:
1. Edit `frontend/app/public/favicon.svg`
2. Run `npm run generate-icons`
3. Review the generated icons
4. Commit all changes (SVG + generated PNGs/ICOs)

### Technical Details

- **Rendering Engine**: Playwright (Chromium)
- **Why not sharp/librsvg?** Server-side SVG renderers don't have access to system fonts and render filters differently than browsers
- **Performance**: ~10-15 seconds to generate all icons (browser launch + rendering)
