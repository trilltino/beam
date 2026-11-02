# Beam visual identity

**Direction:** an understated 1980s retro-futurist interface, with black as the
primary field, electric blue as the secondary signal and purple as a tertiary
accent. This describes the development website preview, not a live launchpad
or an economic promise. Product and fee decisions remain in the [README](../README.md)
and [economics brief](ECONOMICS.md).

Favor the simple, dark feel of an early launchpad: a clear action, readable
sample feed, and a few playful retro details. The hero action uses the visible
bracketed text **[start a new coin]** but opens only a local draft preview;
its adjacent copy must say no token is created. King of the Hill may use a
bold, italic, chunky blue/purple extruded heading. Keep its featured slot
explicitly labeled **No live leader** until evidenced rules and authenticated
data exist. These are original Beam treatments, not copied artwork or claims
about another site's operation.

## Mark and color

The mark is a capital **A without a crossbar**: two mirrored, inward-curving
arch legs meet at one sharp apex. It replaces the former ray-gun picture. Keep
the form recognizable at 32 px; do not add bars, outlines, stars, gradients,
or interior detail to the mark.

| Role | Color | Use |
| --- | --- | --- |
| Primary ink | `#05070d` | Page background and mark on light tiles |
| Raised surface | `#0b1120` | Cards and panels |
| Secondary electric blue | `#4ca7ff` | Inverse mark's left leg, active controls and focus cues |
| Tertiary purple | `#9b72f8` | Inverse mark's right leg and limited accents |
| Main text | `#f1f6ff` | Headings and data on dark surfaces |

Use [`web/public/beam-mark.svg`](../web/public/beam-mark.svg) on a light tile
and [`web/public/beam-mark-inverse.svg`](../web/public/beam-mark-inverse.svg)
on a dark surface. Both have transparent backgrounds and identical geometry.
The inverse asset is also the favicon. Do not place the black mark directly on
black, or add a pale tile behind the inverse mark.

## Interface rules

- Reuse the locally bundled Space Grotesk and DM Sans fonts. Uppercase tracked
  labels and geometric headings may evoke the period; body copy and financial
  numbers stay readable. Align amounts with tabular numerals.
- Use faint grids and orbital arcs only behind decorative hero/CTA content.
  Never put scanlines or strong glow over prices, charts, controls or warnings.
- Keep extruded text confined to decorative headings. The grid and orbital
  effects may be reduced or removed to keep the launch action and preview
  labels prominent; never extrude financial amounts or form labels.
- Keep keyboard focus visible, text contrast high, controls usable on mobile,
  and motion optional under `prefers-reduced-motion`. Avoid flashing effects.
- Preserve the explicit sample-data and unavailable-action labels. Visual
  polish must not imply wallet, RPC, trading, migration or buybacks are live.
- New screens should use the same black/blue/purple hierarchy rather than new
  gradients, remote fonts, icon services or raster versions of the mark.

The preview implementation lives in [`web/src/theme-retro.css`](../web/src/theme-retro.css)
and the existing React layout. Run the [web checks](../web/README.md) and inspect
the mark at 32 px plus desktop/mobile screens when changing these assets.
