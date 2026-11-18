# Steam library artwork

Artwork for the Vessel non-Steam shortcut. `vessel steam add` writes the shortcut but does not copy art, so set these by hand: in Steam, right-click Vessel, then **Manage, Set custom artwork** (or the slot's own menu). You can also copy them into `~/.local/share/Steam/userdata/<account>/config/grid/` under the appid-based names below, then restart Steam.

| File | Size | Steam slot | grid/ name |
| --- | --- | --- | --- |
| `capsule.png` | 600x900 | Library capsule (portrait grid) | `<appid>p.png` |
| `wide.png` | 920x430 | Wide capsule (recent games, horizontal grid) | `<appid>.png` |
| `hero.png` | 3840x1240 | Library hero (game page banner). No logo or text; the mark sits at the right edge | `<appid>_hero.png` |
| `logo.png` | 1280x720, transparent | Logo drawn over the hero | `<appid>_logo.png` |
| `icon.png` | 256x256 | Shortcut icon (Properties, icon) | `<appid>_icon.png` |

The `.svg` next to each PNG is its source. The mark is the one in `apps/desktop/src/assets/vessel-logo.svg`. Only the icons (`icon.png` here and the desktop app icons) put it on a circular `#1B1B1E` (ink-3) backdrop; the capsules and the logo use the bare mark. The wordmark is Geist Mono SemiBold (the app's monospace font), outlined to a path so rendering needs no font. Colours come from `DESIGN.md`: surface `#121215` (ink-2), accent `#62D0A4` (accent-400), text `#F4F4F5`.

Regenerate every PNG, and the desktop app icons, with:

```sh
scripts/render-brand-art.sh
```

It needs `inkscape`, ImageMagick (`magick`) and the desktop devDependencies (`npm ci` in `apps/desktop`, for the Tauri CLI).
