# 01. Tokens and primitives

Source: `forks/t3code/apps/web` (paths below are relative to `apps/web/src` unless they start with `packages/`). Tailwind is v4.3.3 (`pnpm-workspace.yaml:81`); component primitives are Base UI (`@base-ui/react`) styled with Tailwind classes, shadcn/coss style.

## 0. Conventions

- **rem.** The root font size is the *interface font size* setting, default 16px, range 12 to 20 (`appearanceFonts.ts:109`, `packages/contracts/src/settings.ts:168-174`). Every rem value scales with it. All px below assume 16px. Tailwind spacing unit is `0.25rem` = 4px, so `p-3` = 12px, `h-7.5` = 30px.
- **Breakpoint.** Tailwind `sm:` is `min-width: 40rem` in a media query, which uses the browser default 16px, so it is always 640px. A desktop window is wider than that, so **desktop values are the `sm:` values.** Tables give desktop first and the <640px value in parentheses where they differ.
- **Opacity modifiers.** `bg-primary/90` compiles to `color-mix(in oklab, var(--primary) 90%, transparent)`, which is the same color with alpha multiplied by 0.90. Hex values below write that as `#rrggbbaa`. Composite it over whatever is underneath.
- **`--alpha(X / n%)`** in the CSS is the same thing.
- **Default transition** for any `transition-*` utility without a duration: 150ms `cubic-bezier(0.4, 0, 0.2, 1)`.
- **Border.** Everything gets `border-color: var(--color-border)` and `outline-color: ring @ 50%` by default; outline-offset is `-2px` on every element, so 2px focus outlines draw inside the box (`index.css:741-749`).
- **Out-of-gamut colors.** Rows marked `P3` are oklch values outside sRGB. The hex is per-channel clipped, which is what Chromium paints on an sRGB display and matches Tailwind's published hex. Use the hex.
- **Pseudo-element edge.** Many controls have a `::before` at `inset: 0` with `border-radius: radius - 1px` whose box-shadow paints a 1px line over the control's own border: light `0 1px rgb(0 0 0 / 4%)` (darkens the bottom border), dark `0 -1px rgb(255 255 255 / 6%)` (lights the top border). This doc calls it the **edge line**. It is off when disabled, focused, invalid, active, or pressed.

## 1. Color tokens

### 1.1 How a color is chosen

The default theme preference is `"system"` (`hooks/useTheme.ts:43-50`). With `system`, `light` or `dark` there is no `data-theme-id` on `<html>`, so every color comes from `index.css` `:root` (`index.css:1146-1240`) plus the `[data-app-sidebar]` subtree override (`index.css:1267-1312`). Dark mode is the `.dark` class on `<html>`. Named themes (section 6) set `data-theme-id` and replace the tokens through `--app-theme-*` variables (`index.css:1358-1424`).

Utilities read a **contrast-adjusted** copy of text and border tokens (`--color-foreground: var(--contrast-foreground)` etc., `index.css:228-282`). At the default contrast (100) each contrast token equals its raw token, so the tables below are what paints. See 1.7 for other contrast values.

### 1.2 Root tokens (default theme)

`index.css:1146-1240` unless noted. Tailwind palette values from `tailwindcss/theme.css`; `zinc-25` is app-defined as `oklch(99.2% 0 0)` (`index.css:302`).

| token | light | dark | source light / dark | P3 |
|---|---|---|---|---|
| `background` | #fcfcfc | #000000 | zinc-25 / `#000` | |
| `app-chrome-background` (html/body fill) | #fcfcfc | #000000 | = background | |
| `foreground` | #18181b | #f5f5f5 | zinc-900 / neutral-100 | |
| `card` | #ffffff | #111111 | white / mix(srgb, neutral-950 97%, white) | |
| `card-foreground` | #18181b | #f5f5f5 | zinc-900 / neutral-100 | |
| `popover` | #ffffff | #111111 | white / same as card | |
| `popover-foreground` | #18181b | #f5f5f5 | | |
| `surface-raised` | #ffffff33 | #111111 | card @20% / same as card | |
| `primary` | #1b4ed8 | #346bf1 | oklch(0.488 0.217 264) / oklch(0.571 0.21 264) | |
| `primary-foreground` | #ffffff | #ffffff | white | |
| `secondary` | #fafafa | #ffffff08 | zinc-50 / white @3% | |
| `secondary-foreground` | #27272a | #f5f5f5 | zinc-800 / neutral-100 | |
| `muted` | #fafafa | #ffffff08 | zinc-50 / white @3% | |
| `muted-foreground` | #71717b | #a1a1a1 | zinc-500 / neutral-400 | |
| `placeholder` | #71717b | #a1a1a1 | = muted-foreground | |
| `secondary-label` | #71717b | #a1a1a1 | = muted-foreground | |
| `icon-muted` | #71717b | #a1a1a1 | = muted-foreground | |
| `accent` | #f4f4f5 | #ffffff0a | zinc-100 / white @4% | |
| `accent-foreground` | #18181b | #f5f5f5 | zinc-900 / neutral-100 | |
| `message-surface` (user bubble) | #ebebed | #1e1e1e | mix(srgb, zinc-200 55%, zinc-100) / mix(srgb, neutral-950 92%, white) | |
| `message-foreground` | #18181b | #f5f5f5 | = foreground | |
| `message-action` | #1b4ed8 | #346bf1 | = primary | |
| `message-action-foreground` | #ffffff | #ffffff | | |
| `message-action-hover` | #3160db | #2f60d9 | mix(srgb, primary 90%, background) | |
| `error` | #fb2c36 | #fb414a | red-500 / mix(srgb, red-500 90%, white) | |
| `error-foreground` | #c10007 | #ff6467 | red-700 / red-400 | P3 |
| `error-surface` | #fb2c3614 | #fb414a29 | error @8% / @16% | |
| `tool-error-icon` | #fb2c36 | #fca5a5 | = error / literal | |
| `destructive` | #fb2c36 | #fb414a | = error | |
| `destructive-foreground` | #c10007 | #ff6467 | = error-foreground | P3 |
| `border` | #e4e4e7 | #ffffff14 | zinc-200 / white @8% | |
| `input` | #d4d4d8 | #ffffff1c | zinc-300 / white @11% | |
| `ring` | #1b4ed8 | #346bf1 | = primary | |
| `info` | #2b7fff | #2b7fff | blue-500 | P3 |
| `info-foreground` | #1447e6 | #51a2ff | blue-700 / blue-400 | P3 |
| `success` | #00bc7d | #00bc7d | emerald-500 | P3 |
| `success-foreground` | #007a55 | #00d492 | emerald-700 / emerald-400 | P3 |
| `warning` | #fe9a00 | #fe9a00 | amber-500 | P3 |
| `warning-foreground` | #bb4d00 | #ffb900 | amber-700 / amber-400 | P3 |
| `warning-surface` | #fe9a0014 | #fe9a0029 | warning @8% / @16% | P3 |
| `update` | #1b4ed8 | #346bf1 | = primary | |
| `update-foreground` | #1b4ed8 | #51a2ff | = primary / blue-400 | P3 |
| `guest` | #e12afb | #e12afb | fuchsia-500 | |
| `guest-foreground` | #c800de | #f4a8ff | fuchsia-600 / fuchsia-300 | P3 |
| `guest-surface` | #ead0ef | #391f3d | mix(srgb, guest 14%, message-surface) | |
| `sidebar` | #fafafa | #111111 | zinc-50 / = card | |
| `sidebar-foreground` | #18181b | #f5f5f5 | = foreground | |
| `sidebar-muted-foreground` | #71717b | #a1a1a1 | = muted-foreground | |
| `sidebar-control-surface` | #f4f4f5 | #ffffff08 | zinc-100 / = muted | |
| `sidebar-row-hover` | #fcfcfc | #ffffff0a | zinc-25 / = accent | |
| `sidebar-row-active` | #ffffff | #ffffff0a | white / = accent | |
| `sidebar-row-selected` | #ffffff | #ffffff08 | white / = muted | |
| `sidebar-border` | #e4e4e7 | #ffffff14 | = border | |
| `sidebar-icon-color` | #86858e | #8b8b8b | mix(srgb, sidebar-muted-fg 85%, sidebar) (`index.css:104-108`) | |
| `code-background` | #ffffff | #101010 | mix(srgb, card 90%, background) | |
| `code-foreground` | #18181b | #f5f5f5 | = foreground | |
| `terminal-background` | #fcfcfc | #000000 | = background | |
| `terminal-foreground` | #18181b | #f5f5f5 | = foreground | |
| `terminal-cursor` | #26384e | #b4cbff | rgb literals | |
| `terminal-selection-background` | #253f6333 | #b4cbff40 | rgb literals | |
| `toolbar-background` | #fcfcfc | #000000 | = app-chrome-background | |
| `toolbar-foreground` | #18181b | #f5f5f5 | = foreground | |
| `toolbar-border` | #e4e4e7 | #ffffff14 | = border | |
| `toolbar-control` | #ffffff | #111111 | = popover | |
| `toolbar-control-foreground` | #18181b | #f5f5f5 | = foreground | |
| `toolbar-control-hover` | #f4f4f5 | #ffffff0a | = accent | |
| `app-scrollbar-thumb` | #d9d9d9 | #ffffff5c | rgb(217 217 217) / white @36% (`index.css:97,147`) | |
| `app-scrollbar-thumb-hover` | #bfbfbf | #ffffff73 | rgb(191 191 191) / white @45% | |
| find highlight fg | #21180a | #f5f5f5 | `index.css:433-450` | |
| find highlight bg | #ffde8b | #8c6c1f | oklch(0.915 0.11 85) / oklch(0.55 0.1 85) | P3 |
| active find highlight fg | #21180a | #21180a | | |
| active find highlight bg | #eea52b | #eea52b | oklch(0.774202 0.151946 74.331) | |

Dark-mode note in the source: the canvas is true black and so is the app sidebar; cards, popovers and the composer sit at #111111 (`index.css:1219-1222`).

There are **no** chart tokens, no `--chart-*`, no `--sidebar-primary`/`--sidebar-accent` (shadcn's). Do not invent them.

### 1.3 App sidebar subtree (`[data-app-sidebar]`)

The left sidebar element carries `data-app-sidebar` (`components/AppSidebarLayout.tsx:482`) and redefines these for itself and its children (`index.css:1267-1312`). In dark mode this is what makes the sidebar pure black rather than #111111. Root `sidebar-*` tokens in 1.2 still apply to sidebar-like surfaces outside it (settings nav, portaled sheets). Tokens not listed inherit from root. The contrast tokens are recomputed inside this subtree (`index.css:1624-1625`).

| token | light | dark |
|---|---|---|
| `background` | #fcfcfc | #000000 |
| `foreground` | #18181b | #f1f3f7 |
| `card` | #ffffff | #000000 |
| `accent` | #f4f4f5 | #191a1d |
| `accent-foreground` | #18181b | #f7f9ff |
| `muted` | #fafafa | #0a0a0a |
| `muted-foreground` | #71717b | #a3a3a3 |
| `secondary-label` | #71717b | #8f8f8f (row metadata) |
| `border` | #e4e4e7 | #ffffff14 |
| `input` | #d4d4d8 | #ffffff2e (white @18%) |
| `sidebar` | #fafafa | #000000 |
| `sidebar-foreground` | #18181b | #f1f3f7 |
| `sidebar-muted-foreground` | #71717b | #a3a3a3 |
| `sidebar-control-surface` | #f4f4f5 | #0a0a0a |
| `sidebar-row-hover` | #fcfcfc | #f1f3f714 (fg @8%) |
| `sidebar-row-active` | #ffffff | #f1f3f71c (fg @11%) |
| `sidebar-row-selected` | #ffffff | #f1f3f712 (fg @7%) |
| `sidebar-border` | #e4e4e7 | #ffffff14 |

### 1.4 Glass surfaces and backdrops

Defaults: `--glass-opacity` 80% (setting, 40 to 100, applied in `appInit.tsx:101`), `--glass-blur` 12px light / 16px dark, `--glass-saturation` 1.14 light / 1.08 dark (`index.css:124-126,149-150`).

| utility | fill (light / dark) | border | shadow | backdrop | source |
|---|---|---|---|---|---|
| `surface-glass` | background @80%: #fcfcfccc / #000000cc | | | blur 12/16px, saturate 1.14/1.08 | `index.css:391-399` |
| `dropdown-glass` (menus, popovers, selects, comboboxes, toasts) | mix(srgb, popover 18%, popover @80%) = popover @83.6%: #ffffffd5 / #111111d5 | 1px foreground @10%: #18181b1a / #f5f5f51a | | same | `index.css:481-494` |
| `dialog-glass` (dialogs, command palette) | background @80%: #fcfcfccc / #000000cc | light: foreground @10% #18181b1a; dark: white @8% #ffffff14 | light `0 24px 64px -24px rgb(0 0 0/65%)`; dark `inset 0 1px rgb(255 255 255/4%), 0 24px 72px -20px rgb(0 0 0/90%)` | same | `index.css:452-469` |
| `dialog-backdrop` | background @60% #fcfcfc99 / @64% #000000a3 | | | blur 4px | `index.css:471-479` |
| `alert-glass` | surface-glass plus a 4% tint of destructive / info / success / warning by `data-variant` | | | same | `index.css:401-431` |

**Fallback when backdrop blur is unavailable** (defined in the CSS, use it in GPUI if you do not implement element backdrop blur): `surface-glass` and `alert-glass` paint solid `background`; `dropdown-glass` and `dialog-glass` paint solid `popover`.

### 1.5 State colors used by primitives

All are `token @ n%`. Composite over the surface beneath.

| expression | light | dark |
|---|---|---|
| primary/90 (button hover) | #1b4ed8e6 | #346bf1e6 |
| primary/24 (button drop shadow color) | #1b4ed83d | #346bf13d |
| ring/24 (input focus ring) | #1b4ed83d | #346bf13d |
| ring/50 (default outline color) | #1b4ed880 | #346bf180 |
| destructive/90 | #fb2c36e6 | #fb414ae6 |
| destructive/36 (invalid border) | #fb2c365c | |
| destructive/64 (invalid + focus border) | #fb2c36a3 | |
| destructive/16 light, /24 dark (invalid focus ring) | #fb2c3629 | #fb414a3d |
| destructive/8 light, /16 dark (error badge bg) | #fb2c3614 | #fb414a29 |
| destructive/4 (destructive-outline hover bg) | #fb2c360a | |
| destructive/32 (destructive-outline hover border) | #fb2c3652 | |
| accent/50 (outline hover, light) | #f4f4f580 | |
| input/32 (dark control fill) | | #ffffff09 |
| input/48 (dark outline badge hover) | | #ffffff0d |
| input/64 (dark outline hover; toggle pressed) | #d4d4d8a3 | #ffffff12 |
| input/72 (dark segmented pressed) | | #ffffff14 |
| input/40 (segmented track) | #d4d4d866 | #ffffff0b |
| background/55 (segmented hover, light) | #fcfcfc8c | |
| foreground/4 light, /8 dark (autofill) | #18181b0a | #f5f5f514 |
| foreground/6 (command/combobox selected) | #18181b0f | #f5f5f50f |
| foreground/7 (kbd fill) | #18181b12 | #f5f5f512 |
| foreground/8 (select/radio selected, palette kbd) | #18181b14 | #f5f5f514 |
| foreground/9 (command/combobox highlight) | #18181b17 | #f5f5f517 |
| foreground/15 (kbd outline border) | #18181b26 | #f5f5f526 |
| foreground/55 (checkbox `contrast` border) | #18181b8c | #f5f5f58c |
| muted-foreground/15 (skeleton) | #71717b26 | #a1a1a126 |
| muted-foreground/55 (search icon in popups) | #71717b8c | #a1a1a18c |
| muted/72 (dialog footer) | #fafafab8 | #ffffff06 |
| border/60 (glass button, toast close orb) | #e4e4e799 | #ffffff0c |
| border/70 (popup search underline) | #e4e4e7b2 | |
| popover/92 (toast close orb) | #ffffffeb | #111111eb |
| info/8, /16 dark | #2b7fff14 | #2b7fff29 |
| success/8, /16 dark | #00bc7d14 | #00bc7d29 |
| warning/8, /16 dark | #fe9a0014 | #fe9a0029 |
| warning/32, /40, /16, /24 dark | #fe9a0052, #fe9a0066, #fe9a0029 | #fe9a003d |

### 1.6 Diff colors

`index.css:2371-2410`, `index.css:1313-1334`. Setting `diffColorScheme`, default `red-green`.

| token | light | dark |
|---|---|---|
| `diff-addition` (= success) | #00bc7d | #00bc7d |
| `diff-deletion` (= destructive) | #fb2c36 | #fb414a |
| `diff-addition-foreground` | #007a55 (emerald-700) | #00d492 (emerald-400) |
| `diff-deletion-foreground` | #c10007 (red-700) | #ff6467 (red-400) |
| `diff-modified-icon` | #425cb1 (mix srgb info-fg 50%, muted-fg) | #51a2ffa6 (info-fg @65%) |
| `diff-added-icon` | unset (callers fall back) | #00d492 (success-foreground) |

Blue-orange scheme (`:root[data-diff-color-scheme="blue-orange"]`): addition = addition-foreground = light #155dfc (blue-600) / dark #51a2ff (blue-400); deletion = deletion-foreground = light #f54900 (orange-600) / dark #ff8904 (orange-400); `diff-modified-icon` = muted-foreground (light), muted-foreground @65% (dark) = #a1a1a1a6.

Line backgrounds inside diffs come from `@pierre/diffs`, which reads these through `--diffs-addition-color-override` etc.; the diff surface background is `code-background` (`index.css:2441-2446`). That belongs in the diff spec.

### 1.7 Appearance contrast

Setting `appearanceContrast`, integer 50 to 200, default 100 (`packages/contracts/src/settings.ts:150-156`). `appearanceContrast.ts:3-10` writes three variables on `<html>`:

| var | formula | at 100 |
|---|---|---|
| `--appearance-contrast-base` | min(c, 100)% | 100% |
| `--appearance-contrast-boost` | max(c - 100, 0)% | 0% |
| `--appearance-contrast-border-boost` | max(c - 100, 0) / 4 % | 0% |
| `--appearance-contrast-target` | black in light, white in dark (`index.css:95,146`) | |

Derived tokens (`index.css:1624-1736`):

- **Text tokens**, each against its own surface, mixed in **oklab**: `contrast-X = mix(oklab, mix(oklab, X base, SURFACE), target boost)`.
  - foreground, muted-foreground, placeholder, secondary-label, icon-muted: surface = background
  - card-foreground/card, popover-foreground/popover, accent-foreground/accent, secondary-foreground/secondary, message-foreground/message-surface, sidebar-foreground/sidebar, sidebar-muted-foreground/sidebar, toolbar-foreground/toolbar-background, toolbar-control-foreground/toolbar-control
- **Line tokens**, mixed in **sRGB**: `contrast-X = mix(srgb, mix(srgb, X base, transparent), FG border-boost)`.
  - border, input (FG = foreground); sidebar-border (FG = sidebar-foreground); toolbar-border (FG = toolbar-foreground)

So below 100, text fades toward its surface and lines become translucent; above 100, text moves toward black/white by `c - 100`% and lines move toward the foreground by a quarter of that. Fills (backgrounds, primary, status colors) never change.

### 1.8 Surface grain

Body and opt-in chrome surfaces get a noise texture over their fill: SVG `feTurbulence type=fractalNoise baseFrequency=0.9 numOctaves=4 stitchTiles=stitch`, rect opacity 0.035, tiled at 256x256 px (`index.css:1793-1811`). The T3 Chat theme uses a 128px tile at 0.02745. To match pixels, bake a 256x256 grayscale fractal-noise tile once and draw it at 3.5% over `app-chrome-background`. The `surface-grain` utility (`index.css:1131-1135`) applies the same tile to floating chrome.

## 2. Typography

`index.css:172-216`, `appearanceFonts.ts`, `main.tsx:4`.

| role | stack | notes |
|---|---|---|
| sans (`--font-sans`) | `-apple-system, BlinkMacSystemFont, "Inter Variable", "Segoe UI", system-ui, sans-serif` | Inter Variable is bundled (`@fontsource-variable/inter`, `main.tsx:4`). macOS renders SF Pro; Linux and Windows render Inter Variable. Ship Inter Variable with Tern. |
| mono (`--font-mono`) | `ui-monospace, "SF Mono", "SFMono-Regular", Menlo, Consolas, "Liberation Mono", monospace` | Chromium resolves `ui-monospace` only on macOS (SF Mono). On Linux this lands on Liberation Mono if installed, else the system monospace. A custom code font is prepended to `"SF Mono", "SFMono-Regular", Menlo, Consolas, "Liberation Mono", monospace` (`appearanceFonts.ts:17-18`). |
| composer (`--font-composer`) | = sans unless "separate fonts" is on | |
| diff header | sans; diff body mono (`index.css:1830-1833`) | |

Font settings and defaults (`packages/contracts/src/settings.ts:168-190,499-515`): interface 16px (12 to 20, sets root size), prompt 14px (12 to 20; when fonts are not "separate", `--font-size-prompt` is `var(--text-sm)` = 14px), code 13px (10 to 18, `--font-size-code` and `--diffs-font-size`), terminal 12px (8 to 20). Font smoothing on by default (`-webkit-font-smoothing: antialiased`, i.e. grayscale AA on macOS).

Base: body inherits 16px / line-height 1.5 (Tailwind preflight). Almost all UI text sets its own size.

**Size scale** (custom steps at `index.css:186-193`, rest Tailwind default). Frequency is count of class uses in `src/**/*.tsx`.

| class | size | line height | uses |
|---|---|---|---|
| `text-5xs` | 7px | 7px (1) | 6 (provider icon overlays) |
| `text-4xs` | 8px | 8px (1) | 3 |
| `text-3xs` | 10px | 14px | 74 |
| `text-2xs` | 11px | 16px | 172 |
| `text-xs` | 12px | 16px | 1016 |
| `text-sm` | 14px | 20px | 525 (+31 `sm:text-sm`) |
| `text-base` | 16px | 24px | 49 (mostly `text-base sm:text-sm`, i.e. 14px on desktop) |
| `text-lg` | 18px | 28px | 7 |
| `text-xl` | 20px | 28px | 6 (dialog titles) |
| `text-2xl` | 24px | 32px | 10 |
| `text-3xl` | 30px | 36px | 4 |
| `text-[11px]` | 11px | inherited | button `micro`, tooltip `code` |

Leading utilities: `leading-none` 1, `leading-tight` 1.25, `leading-snug` 1.375, `leading-normal` 1.5, `leading-relaxed` 1.625; numeric `leading-N` = N x 4px (`leading-4` 16px, `leading-4.5` 18px, `leading-5` 20px).

Weights: `font-normal` 400, `font-medium` 500 (450 uses; the default for controls, labels, titles in menus), `font-semibold` 600 (121; dialog/sheet titles), `font-bold` 700 (3). Tracking: `tracking-widest` 0.1em (keyboard shortcut text), `tracking-tight` -0.025em, `tracking-wide` 0.025em.

`font-variant-ligatures: none` on URL and script-command inputs (`index.css:1745-1748`).

## 3. Geometry, elevation, motion

### 3.1 Radii

`--radius` = 0.5rem = 8px (`index.css:1148`); Tailwind radius keys are rebased on it (`index.css:283-288`).

| class | px |
|---|---|
| `rounded-xs` | 2 (Tailwind default, not rebased) |
| `rounded` (bare) | 4 (Tailwind reference value, ignores `--radius`) |
| `rounded-sm` | 4 (radius - 4) |
| `rounded-md` | 6 |
| `rounded-lg` | 8 |
| `rounded-xl` | 12 |
| `rounded-2xl` | 16 |
| `rounded-3xl` | 20 |
| `rounded-full` | 9999 |
| `--control-radius` | 8 (buttons, input groups) |

Inner `::before` layers use radius - 1px.

### 3.2 `:root` layout variables

`index.css:91-158`.

| var | value |
|---|---|
| `--app-scrollbar-width` | 6px |
| `--control-radius` | 8px |
| `--sidebar-content-inset` | 8px |
| `--sidebar-control-gap` | 8px |
| `--sidebar-row-content-inset` | 10px |
| `--command-shell-inset` | 8px |
| `--command-content-inset` | 16px |
| `--floating-content-inset` | 12px |
| `--z-sheet` | 46 |
| `--workspace-topbar-height` | 52px (with Window Controls Overlay: `max(40px, titlebar-area-height)`) |
| `--workspace-controls-left` / `-right` | 12px (+ safe area) |
| `--workspace-titlebar-control-size` | 28px |
| `--workspace-titlebar-control-gap` | 12px |
| `--workspace-titlebar-scroll-fade-height` | 24px |
| `--chat-content-max-width` | 46rem = 736px (`wide` 72rem = 1152px, `full` 100%; `index.css:2380-2387`) |
| `--html-preview-max-width` | = chat max width |
| `--thread-details-panel-width` | 17.5rem = 280px |
| `--workspace-gutter` | 12px (<640), 20px (>=640) |
| `--desktop-window-right-resize-inset` | 0, 6px on Windows Electron (`index.css:1782-1784`) |

### 3.3 Shadows

| name | value | source |
|---|---|---|
| `shadow-composer` (light) | `0 1px 2px rgb(0 0 0 / 5%)` | `index.css:206` |
| `shadow-composer-dark` | `0 14px 32px -18px rgb(0 0 0 / 75%)` | `index.css:207` |
| composer seam | dark: 10px gradient `rgb(0 0 0/18%)` fading away from a 16px seam; `--spacing-composer-seam` = 15px | `index.css:208-226` |
| `inset-shadow-control-highlight` | `inset 0 1px rgb(255 255 255 / 16%)` | `index.css:203` |
| `inset-shadow-control-pressed` | `inset 0 1px rgb(0 0 0 / 8%)` | `index.css:204` |
| popup (menu, popover, select, combobox) | light `0 16px 40px -18px rgb(0 0 0/55%)`, dark `0 18px 44px -18px rgb(0 0 0/80%)` | `components/ui/menu.tsx:58` |
| dialog | see `dialog-glass`, 1.4 | |
| `shadow-xs/5` | `0 1px 2px 0 rgb(0 0 0/5%)` | Tailwind; alpha modifier replaces the shadow alpha |
| `shadow-sm` | `0 1px 3px 0 rgb(0 0 0/10%), 0 1px 2px -1px rgb(0 0 0/10%)` | |
| `shadow-sm/5` | same geometry at 5% | |
| `shadow-md/5` | `0 4px 6px -1px rgb(0 0 0/5%), 0 2px 4px -2px rgb(0 0 0/5%)` | |
| `shadow-lg/5` | `0 10px 15px -3px rgb(0 0 0/5%), 0 4px 6px -4px rgb(0 0 0/5%)` | |
| `shadow-xl shadow-black/25` | `0 20px 25px -5px rgb(0 0 0/25%), 0 8px 10px -6px rgb(0 0 0/25%)` | |
| edge line | light `0 1px rgb(0 0 0/4%)`, dark `0 -1px rgb(255 255 255/6%)` on `::before` | section 0 |

### 3.4 Focus ring

Two shapes are used:

- **Ring** (`focus-visible:ring-2 ring-inset ring-ring ring-offset-1 ring-offset-background`): inside the border, a 1px band of `background` at the outer edge, then a 2px band of `ring` (primary) inward. Buttons, badges, toggles, switches, scroll viewports.
- **Field ring** (inputs, textarea, select trigger, input group): border turns `ring`, plus a 3px inset band of `ring @24%`. Drop shadow off.
- **Outline** (checkbox, inline button): 2px solid `ring`, offset -2px.

### 3.5 Z order

`index.css:115-118` plus component classes.

| layer | z |
|---|---|
| bottom terminal drawer | 45 |
| sheets, details panel popover | 46 (`--z-sheet`) |
| floating preview player | 47 to 49 |
| dialog backdrop, dialog viewport, command palette | 50 |
| media dialog | 60 |
| toast viewport, theme inspector | 100 (inspector hover 101, editor panel 120) |
| menu, popover, select, combobox, autocomplete | 130 |
| tooltip | 140 |

### 3.6 Motion

`index.css:196-205,230-344`.

| name | value |
|---|---|
| default transition | 150ms `cubic-bezier(0.4,0,0.2,1)` |
| `ease-drawer` | `cubic-bezier(0.32, 0.72, 0, 1)` |
| `animate-skeleton` | 2.4s infinite; opacity 1 (0 to 42%), 0.55 (50 to 92%), back to 1; `steps(4)` ramps |
| `animate-status-pulse` | 2s infinite; opacity 1 (0 to 40%), 0.5 (50 to 90%); `steps(6)` |
| `animate-status-ping` | 2s infinite; at 0% opacity 0.9 scale 0.75, by 40% opacity 0 scale 2; `steps(8)` |
| `animate-spin` | 1s linear infinite |
| `animate-step-in` | 180ms ease-out from opacity 0, translateY 4px |
| `animate-step-ping` | 520ms ease-out, opacity 0.55 to 0, scale 0.7 to 1.45 |
| `animate-check-draw` | 200ms ease-out, stroke-dashoffset 12 to 0 |
| `live-tool-shine` | 2.2s `steps(30)` infinite; a 72px gradient highlight sweeps across text |
| panel open/close | setting `panelAnimationDurationMs`, default **0** (instant), max 400 |
| theme switch | `.no-transitions` on `<html>` for one frame: all transitions and animations 0s (`index.css:1138-1144`, `hooks/useTheme.ts:327-345`) |

Indicators are deliberately stepped (discrete frames), not smooth.

### 3.7 Scrollbars

- **Native** (`index.css:1876-1891`): 6px wide, transparent track, thumb `app-scrollbar-thumb`, radius 3px, hover `app-scrollbar-thumb-hover`.
- **ScrollArea overlay** (`components/ui/scroll-area.tsx:89-108`): vertical bar 6px wide, 4px inset top and bottom, 1px from the right edge; horizontal bar 6px tall, 4px inset left and right, 1px from bottom. Thumb fully rounded, same colors. Hidden (opacity 0) until hover or scroll, fades in over 100ms with no delay, fades out over 150ms after a 300ms delay.
- **Scroll fade** (`scrollFade` prop): 24px mask fade at any edge that has more content (`--fade-size: 1.5rem`). The titlebar fade is 24px tall with stops 0, 10%, 30%, 58%, 82%, 96%, 100% alpha (`index.css:496-535`).

## 4. Primitives

All in `components/ui/`. Desktop sizes first, (<640px) in parentheses. Disabled is `opacity: 0.64` and no pointer events for every control. Unless noted, controls are 1px-bordered, `font-medium`, and icons inside them get -2px horizontal margin.

### 4.1 Button (`button.tsx:8-73`)

Base: inline-flex, centered, gap 8px, `rounded` = `--control-radius` 8px, 1px border, 14px (16px) medium, no wrap. Icon default 16px (18px), color `--control-icon-color` (currentColor unless the variant sets it). Press: `scale(0.97)` unless it opens a popup. Transition box-shadow and scale, 150ms. Focus: ring (3.4).

**Sizes** (height includes the border; horizontal padding is written as `spacing - 1px` to absorb it):

| size | height | padding x | gap | text | icon | radius |
|---|---|---|---|---|---|---|
| `default` | 32 (36) | 11 | 8 | 14 | 16 | 8 |
| `xs` | 24 (28) | 7 | 6 | 12 (14) | 14 (16) | 8 |
| `sm` | 28 (32) | 9 | 6 | 14 | 16 | 8 |
| `lg` | 36 (40) | 13 | 8 | 14 | 16 | 8 |
| `xl` | 40 (44) | 15 | 8 | 16 (18) | 18 (20) | 8 |
| `compact` | 28 | 7 | 6 | 12 | 14 | 6 |
| `micro` | 20 | 5 | 6 | 11 | 12 | 4 |
| `multiline` | min 32 (36), py 5 | 11 | 8 | 14 | 16 | 8, wraps |
| `sm-multiline` | min 28 (32), py 3 | 9 | 6 | 14 | 16 | 8, wraps |
| `sm-pill` | 28 (32) | 10 | 6 | 14 | 16 | full |
| `icon` | 32x32 (36) | 0 | | | 16 | 8 |
| `icon-sm` | 28x28 (32) | | | | 16 | 8 |
| `icon-sm-pill` | 28x28 (32) | | | | 16 | full |
| `icon-xs` | 24x24 (28) | | | | 14 (16); 16 inside an input group | 8 |
| `icon-lg` | 36x36 (40) | | | | 16 | 8 |
| `icon-xl` | 40x40 (44) | | | | 18 (20) | 8 |
| `icon-micro` | 20x20 | 0 | | | 12 | 4 |
| `icon-tiny` | 16x16 | 0 | | | 12 | 8 |

**Variants.** "hover" also applies while `data-pressed` (menu trigger open).

| variant | rest | hover | active / pressed | icon |
|---|---|---|---|---|
| `default` | bg + border `primary`, text `primary-foreground`; shadow `inset 0 1px white/16%` + `0 1px 2px primary/24%` | bg primary/90 | inset `0 1px black/8%`, no drop shadow | currentColor |
| `destructive` | same recipe with `destructive`, text white | bg destructive/90 | same | currentColor |
| `outline` | border `input`; bg `popover` (dark: input/32 = #ffffff09); text foreground; `shadow-xs/5`; edge line | bg accent/50 (dark input/64) | no shadow, no edge line | contrast-muted-foreground |
| `destructive-outline` | as outline, text `destructive-foreground` | border destructive/32, bg destructive/4 | as outline | currentColor |
| `secondary` | bg `secondary`, transparent border, text `secondary-foreground` | bg secondary/90 | bg secondary/80 (both rules match when pressed; I could not confirm which wins without a build) | currentColor |
| `ghost` | transparent, text foreground | bg accent | bg accent | contrast-muted-foreground |
| `ghost-muted` | transparent, text muted-foreground | bg accent, text foreground | same | currentColor |
| `ghost-destructive` | transparent, text muted-foreground | bg accent, text destructive | same | currentColor |
| `glass` | `surface-glass`, radius full, border border/60, text foreground, `shadow-sm` | border `border` | | contrast-muted-foreground |
| `link` | transparent | underline, offset 4px | | |
| `warning-outline` | border warning/32, bg `warning-surface`, text warning-foreground, `shadow-xs/5` | border warning/40, bg warning/16 (dark /24) | no shadow | |
| `overlay` | bg black/70, text white/65 | bg black/90 | | |
| `media-close` | bg black/65, text white, `shadow-sm`, 1px ring white/20 | bg black/80 | | focus ring white |
| `media-navigation` | absolute, vertically centered, z 20, text white/90 | bg white/10, text white | | |

**InlineButton** (`button.tsx:103-139`): text-only, `font-medium`, gap 2px, underline on hover (offset 2px), focus outline 2px ring. Tones: `default` foreground; `muted` muted-foreground to foreground on hover; `destructive` destructive/80 to destructive; `picker` gap 4px, underline offset 4px, underlined while its popup is open.

### 4.2 Input (`input.tsx`)

Wrapper span: radius 8, 1px border `input`, bg `background` (dark input/32), text 14 (16), `shadow-xs/5`, edge line.

| size | inner height | total height | padding x | text | radius |
|---|---|---|---|---|---|
| `default` | 30 (34) | 32 (36) | 11 | 14 (16) | 8 |
| `sm` | 26 (30) | 28 (32) | 9 | 14 | 8 |
| `lg` | 34 (38) | 36 (40) | 11 | 14 | 8 |
| `compact` | 28 | 30 | 9 | 12 | 6 |

Line height equals inner height (single line, vertically centered). Placeholder `placeholder`. States: focus = field ring (3.4); invalid border destructive/36, invalid+focus border destructive/64 with ring destructive/16 (dark /24); autofill bg foreground/4 (dark /8); disabled 0.64. `font="mono"` adds mono + tabular numbers.

### 4.3 Textarea (`textarea.tsx`)

Same wrapper as Input. Auto-grows with content (`field-sizing: content`), max height 256px.

| size | padding | min height |
|---|---|---|
| `default` | 5 / 11 | 70 (82) |
| `sm` | 3 / 9 | 66 (78) |
| `lg` | 7 / 11 | 74 (86) |

With `rows`, min height = rows x line-height + 2 x vertical padding.

### 4.4 Input group (`input-group.tsx`)

Container: radius 8, 1px border, text 14 (16); the inner input loses its own border. Variants: `default` = Input wrapper look; `popover` = `dropdown-glass`, radius 16, on focus no ring and border foreground @18%; `ghost` = transparent, hover bg muted/40, focused bg background. Size `lg` height 36. Inline addons pad 4px at the outer side, gap 2px; block addons pad 11px. Kbd inside addons radius 3px. Textarea inside: min height 82 (94), py 11, no resize.

### 4.5 Badge (`badge.tsx`)

Inline-flex, gap 4, radius 4 (`rounded-sm`), transparent border, medium, icons 12px (14px) at 80% opacity.

| size | height / min-width | padding x | text | radius |
|---|---|---|---|---|
| `default` | 18 (22) | 3 | 12 (14) | 4 |
| `sm` | 16 (20) | 3 | 10 (12), leading 1 | 4 |
| `lg` | 22 (26) | 5 | 14 (16) | 4 |
| `control` | 24 (28) | 7 | 12 (14) | 8 |

| variant | bg | text | hover (when a link/button) |
|---|---|---|---|
| `default` | primary | primary-foreground | primary/90 |
| `destructive` | destructive | white | destructive/90 |
| `secondary` | secondary | secondary-foreground | secondary/90 |
| `outline` | background (dark input/32), border input | foreground | accent/50 (dark input/48) |
| `error` | destructive/8 (dark /16) | destructive-foreground | |
| `info` | info/8 (dark /16) | info-foreground | |
| `success` | success/8 (dark /16) | success-foreground | |
| `warning` | warning/8 (dark /16) | warning-foreground | |
| `label` | `--label` @8% (dark @12%) | mix(srgb, label 30%, foreground) (dark 45%) | |

### 4.6 Kbd (`kbd.tsx`)

Height 20, min-width 20, padding x 4, radius 4, gap 4, text 12 medium **sans**, color muted-foreground, icons 12px, not selectable. `default`: bg foreground/7. `outline`: 1px border with 2px bottom border, border foreground/15, padding x 6, no fill. `KbdGroup`: inline-flex gap 4. In menus the shortcut is plain text instead: 12px medium, `secondary-label`, letter-spacing 0.1em, right-aligned (`menu.tsx:303`).

### 4.7 Menu / dropdown (`menu.tsx`)

| part | spec |
|---|---|
| positioner | z 130; default side bottom, align center, offset 4px |
| popup | `dropdown-glass`, radius 8, popup shadow (3.3), min width 160px, max width 100vw - 32px, `width="content"` makes it fit content. **No open/close animation**; it appears and vanishes. |
| scroll body | padding 4, max height = available height, vertical scroll |
| item | min height 28 (32), padding 4 / 8, gap 8, radius 4, text 14 (16), color foreground. Icon 16px (18), 80% opacity, muted-foreground. Highlighted: bg accent, text accent-foreground. `destructive`: text destructive-foreground, icon currentColor. `inset`: padding-left 32. `density="touch"`: min height 40. `variant="ghost"`: button ghost/compact look, min height 28, text 12. |
| checkbox item | grid `16px 1fr`, padding-left 8, right 16. Checkmark is a 24-viewBox path `M5.252 12.7 10.2 18.63 18.748 5.37`, stroke 1.75, round caps. `variant="icon"`: column 10px, mark 14px. `variant="switch"`: label then a mini switch: track 14x22, 1px padding, thumb 12px, checked translate 8px (mobile thumb 16), track primary/input, inset `0 1px black/4%`. |
| radio item | min height 28, padding 4 / 8, radius 4; checked bg foreground/8; check icon 14px at the end |
| group label | padding 6 / 8, text 12 medium muted-foreground; inset 32 (36) |
| separator | 1px, bg border, margin 4px vertical / 8px horizontal |
| shortcut | see Kbd |
| submenu trigger | as item; open state bg accent; trailing chevron-right, 80% opacity. Submenu opens inline-end, offset 0, align-offset -5px |
| popup search field | padding 6 / 8 top, 4 bottom; underline 1px border/70, turns `ring` on focus; search icon 16px muted-foreground/55; input height 26, indent 20, text 14 |

### 4.8 Popover (`popover.tsx`)

| part | spec |
|---|---|
| positioner | z 130; side bottom, align center, offset 4 |
| popup | `dropdown-glass`, radius 8, popup shadow, edge line; text popover-foreground. Calendar inside: radius 12, padding 8. |
| width | `auto` (content), `sm` 256, `md` 320, `lg` 384 (capped at 100vw - 32) |
| padding | `default` 16 all round; `compact` 8 vertical / 12 horizontal; `none` 0 (inner radius 7) |
| animation | open from scale 0.98 + opacity 0 to 1/1; close reverses; 150ms default easing. Size changes animate width/height too. |
| `tooltipStyle` | radius 6, text 12, `shadow-md/5`, padding 4 / 8 |
| `variant="panel"` | no chrome (transparent, no border, no shadow, no blur), z 46, width min(280 + 16, anchor width - 8), padding 8, top transition 150ms ease-out |
| title | 14px semibold, line-height 1 |

### 4.9 Tooltip (`tooltip.tsx`, `shared-tooltip.tsx`)

| part | spec |
|---|---|
| positioner | z 140, side top (Base UI default), offset 4px, collision padding 5px |
| popup | radius 6, text 12, line-height 1.375, max width 320, wraps anywhere; padding 4 / 8 |
| `default` | 1px border `border`, bg popover, `shadow-md/5`, edge line |
| `glass` | `dropdown-glass` + `shadow-xl` black/25, no edge line |
| `code` | max width 480, mono 11px, line-height 1.625, left aligned |
| animation | scale 0.98 + opacity 0 to 1, 150ms; instant when moving between tooltips |
| timing | open after 600ms rest; reopens instantly within 400ms of another closing; 100ms close grace for hoverable tooltips (`shared-tooltip.tsx:18-22`). Sidebar uses 150ms (`components/Sidebar.tsx:3349`); a few settings rows 200 to 350ms. |

### 4.10 Dialog (`dialog.tsx`, `dialog-styles.ts`)

| part | spec |
|---|---|
| backdrop | fixed, z 50, `dialog-backdrop` (background @60% / @64% + 4px blur); fades 200ms |
| viewport | fixed, z 50, padding 16, grid rows `1fr auto 1fr`: centered vertically and horizontally |
| popup | `dialog-glass`, 1px border, radius 16, max width 512 (`max-w-lg`), max height 100%, text popover-foreground, column flex |
| enter/exit | scale 0.98 + opacity 0, 200ms `ease-in-out` (`cubic-bezier(0.4,0,0.2,1)`) |
| nested | each open child pushes the parent up 20px, scale -0.1, opacity -0.1; the child enters from 32px below |
| close button | absolute top 8 / right 8, Button `ghost` `icon` (32x32), lucide `X` |
| header | padding 24, column gap 8 (bottom padding 12 when a panel follows) |
| title | 20px semibold, line-height 1.375 |
| description | 14px muted-foreground |
| panel | padding 24, children spaced 16; top padding 4 after a header |
| footer `default` | border-top, bg muted/72, padding 16 / 24, buttons right-aligned in a row with gap 8, bottom radius 15 |
| footer `bare` | padding 16 / 24, no border or fill |
| media variant | backdrop black/75, no blur, z 60; popup transparent, no border, max 92vw x 92vh |

Initial focus goes to the first enabled `dialog-action` button.

### 4.11 Alert dialog (`alert-dialog.tsx`)

Same backdrop, viewport, popup and animation as Dialog, max width 512. Header padding 24, gap 8, text centered below 640px and left-aligned above. Title 20px semibold, line-height 1.375. Description 14px muted-foreground, selectable, scrolls past min(384px, 60vh). Footer `default` as Dialog; `bare` bottom padding 24. No close button.

### 4.12 Sheet (`sheet.tsx`)

| part | spec |
|---|---|
| backdrop | z 46, background @60% + 4px blur (`backdrop-blur-xs`), fade 200ms |
| popup | bg popover, text popover-foreground, `shadow-lg/5`, edge line (hidden below 640) |
| right (default) / left | width 100% - 48px, max 448px, border on the inner side; slides 32px + fades, 200ms ease-in-out |
| top / bottom | full width, border on the inner side, slides 32px vertically; the opposite side keeps a 48px gap |
| `inset` | 16px margin on desktop, radius 16, full border |
| close button | as Dialog |
| header / title / description | as Dialog |
| `transitionDurationMs` | overrides 200ms; 0 = instant |

### 4.13 Select (`select.tsx`)

**Trigger:** radius 8, 1px border, text 14 (16), gap 8, chevron-down 14px at 70% opacity with -4px end margin, icons `icon-muted` at 80%.

| size | height | padding x | text | radius |
|---|---|---|---|---|
| `default` | min 32 (36) | 11 | 14 | 8 |
| `sm` | min 28 (32) | 9 | 14 | 8 |
| `lg` | min 36 (40) | 11 | 14 | 8 |
| `xs` | 24 (28) | 7 | 12 (14) | 6 |
| `compact` | 28 | 7 | 12 | 6 |

Variant `default`: full width, min width 144, Input wrapper colors, edge line, field ring on focus, invalid as Input. Variant `ghost`: transparent border, text secondary-label; hover or open bg accent, text foreground; focus ring 2px. Placeholder text `placeholder`.

**Popup:** z 130, `alignItemWithTrigger` on (the list is positioned so the selected row sits over the trigger, macOS style), otherwise offset 4. `dropdown-glass`, radius 8, popup shadow, list padding 4. Scroll arrows: 24px tall bands with a popover-colored gradient and a 16px chevron. **Item:** min height 28 (32), padding 4 / 8, radius 4, text 14; selected bg foreground/8; highlighted bg accent, text accent-foreground. No checkmark. Group label as Menu.

### 4.14 Switch (`switch.tsx`)

`--thumb-size`: default 16 (20), `sm` 14 (16). Track height = thumb + 2, width = 2 x thumb - 2, padding 2, radius full. Thumb = thumb - 2 square, fully rounded.

| size | track | thumb | checked travel |
|---|---|---|---|
| default | 30 x 18 (38 x 22) | 14 (18) | 12 (16) |
| `sm` | 26 x 16 (30 x 18) | 12 (14) | 10 (12) |

Track checked `primary`, unchecked `input`, mixed `input` with the thumb centered at 70% opacity. Thumb bg `background`, dark unchecked `muted-foreground`, `shadow-sm/5`. Track color 200ms; thumb translate 150ms, radius 150ms, scale 100ms after 100ms. Pressing stretches the thumb to 110% width toward the travel direction. Focus ring 2px.

### 4.15 Checkbox (`checkbox.tsx`)

16x16 (18), radius 4, 1px border `input`, bg `background` (dark unchecked input/32), `shadow-xs/5`, edge line when unchecked. Checked: an indicator 1px larger on every side fills `primary` and draws the mark in `primary-foreground`: 24-viewBox path `M5.252 12.7 10.2 18.63 18.748 5.37`, stroke 3, round caps, rendered at 12px (14). Indeterminate: path `M5.252 12h13.496`, color foreground. Focus: 2px `ring` outline, offset -2px, plus a 3px `background` inset gap on the checked fill. Invalid border destructive/36. `contrast` variant border foreground/55.

### 4.16 Toggle (`toggle.tsx`)

Base: radius 8, 1px border, 14px (16) medium, foreground, gap 8, icons 16px (18) at 80% opacity. Hover bg accent. Pressed bg input/64, text accent-foreground. Focus ring 2px.

| size | height / min width | padding x | text | radius |
|---|---|---|---|---|
| `default` | 32 (36) | 7 | 14 | 8 |
| `sm` | 28 (32) | 5 | 14 | 8 |
| `xs` | 24 (28) | 3 | 14 | 6 |
| `lg` | 36 (40) | 9 | 14 | 8 |
| `compact` | 28 | 3 | 12, icon 14 | 6 |
| `segmented` | 24, min width 0 | 10 | 12 | 6 |

| variant | look |
|---|---|
| `default` | transparent border |
| `pill` | radius full, padding x 12, `font-normal`, muted-foreground; pressed foreground |
| `ghost` | no shadows; pressed bg accent, text accent-foreground; disabled stays opaque with muted-foreground text |
| `outline` | border input, bg background (dark input/32), `shadow-xs/5`, edge line; dark hover input/64, dark pressed input |
| `segmented` | muted-foreground, color transition; hover bg background/55 + foreground (dark input/32); pressed bg background + foreground + `shadow-xs/10` (dark input/72) |

### 4.17 Toggle group (`toggle-group.tsx`)

Default variant is `segmented`: a track with bg input/40, radius 8, padding 2, gap 2, items size `segmented` (24 tall). So a segmented control is 28px tall overall. Variant `default`: gap 2, no track. `outline`: items fused (inner radii and inner borders removed), horizontal or vertical.

### 4.18 Tabs

There is no Tabs primitive in `components/ui/`, and nothing imports `@base-ui/react/tabs`. Tab-like controls are ToggleGroup `segmented`. Panel tabs use `panel-tab-close-button.tsx`: 16x16 close target, radius 4, hover bg muted, 12px `X` that shows on tab hover.

### 4.19 Separator (`separator.tsx`)

1px line, bg `border`. Horizontal full width; vertical stretches to the parent's height.

### 4.20 Skeleton (`skeleton.tsx`)

bg muted-foreground/15 (#71717b26 / #a1a1a126), `animate-skeleton` (3.6). Shapes: `block` radius 4 (default), `card` radius 8, `pill` full.

### 4.21 Spinner (`spinner.tsx`)

lucide `LoaderCircle`, spinning 1s linear forever, paused when offscreen. Sizes `xs` 12, `sm` 14, `md` 16, `lg` 20; no default (inside a button it takes the button's icon size). Tone `muted` = muted-foreground. `RefreshIcon` (`refresh-icon.tsx`) is the same idea with `RefreshCw`.

### 4.22 Toast (`toast.tsx`)

| part | spec |
|---|---|
| viewport | fixed, z 100, position **top-right** (`routes/__root.tsx:144`); inset 32 (16); top = inset + topbar height = 84px; width min(100% - 2 x inset, 360) |
| toast | `dropdown-glass`, radius 8, `shadow-xl` black/25, text 14, popover-foreground |
| content | padding left 14; one line: padding 12 vertical, right 14 (24 with trailing controls), row with gap 6; with stacked actions: column, gap 8, padding 10 / 14 |
| icon | 16px wide, line-height tall; error `CircleAlert` destructive, info `Info` info, success `CircleCheck` success, warning `TriangleAlert` warning, loading Spinner at 80% |
| title / description | title medium; description muted-foreground, selectable; error text of 180+ chars clamps to 4 lines |
| action | Button size `xs` |
| close orb | 24px circle at top -6 / right -6, border border/60, bg popover/92 (hover popover), muted-foreground (hover foreground), `shadow-sm`, 8px blur, `X` 12px stroke 2.25 |
| stacking | collapsed: each older toast 12px further back and scale 1 - 0.1 x index, height matches the front toast; hover expands into a list with 12px gaps |
| motion | transform 500ms `cubic-bezier(0.22,1,0.36,1)`, opacity 500ms, height 150ms; enters from the right edge (translateX 100% + inset); swipe right or up to dismiss |
| timeout | Base UI default, which I believe is 5000ms; not set in app code and I could not verify without installed packages |

### 4.23 ScrollArea (`scroll-area.tsx`)

Root fills its parent and inherits its radius. Viewport scrolls both axes with contained overscroll; focus ring 2px when focused. Scrollbars and fades: see 3.7. Options: `scrollbarGutter` reserves gutter space, `hideScrollbars` hides them.

### 4.24 Command palette (`command.tsx`)

| part | spec |
|---|---|
| backdrop | as Dialog |
| viewport | fixed, z 50, column, centered horizontally, padding x 16; top padding max(10vh, native titlebar + 8); bottom 10vh |
| popup | `dialog-glass` + Dialog animation, radius 16, max width 576 (`max-w-xl`), max height 420 |
| input row | padding 6 vertical / 8 horizontal; Input size `lg` (34 inner, 36 total) with no border, fill, shadow or ring; search icon `icon-muted` 16px; text indent 32 |
| list | padding 8, scroll padding 8 |
| panel | radius 12 top; 16 bottom when there is no footer; scrollbar inset 8 from top |
| item | min height 28 (32), padding 6 / 8, gap 8, radius 4, text 14; selected bg foreground/6; highlighted bg foreground/9, text foreground; `active` prop forces bg accent |
| group label | padding 6 / 8, 12px medium muted-foreground; adjacent groups 6px apart |
| shortcut | 12px medium secondary-label, letter-spacing 0.1em, right-aligned |
| footer | border-top foreground/7, bg foreground/2.5, padding 10 / 16, 14px medium muted-foreground, kbd bg foreground/8 text foreground; bottom radius 15 |

Always open, first row always highlighted.

### 4.25 Combobox (`combobox.tsx`) and Autocomplete (`autocomplete.tsx`)

**Combobox popup:** z 130, `dropdown-glass`, radius 8, popup shadow, min width = anchor width, max height min(available, 368px). List padding 4. Item min height 28 (32), padding 4 / 8, radius 4, text 14, icons muted-foreground; hover and highlighted bg foreground/9; selected bg foreground/6. Group label padding 8 / 12, 12px medium muted-foreground. Empty: padding 8, centered 14px muted-foreground. Search field: padding 10 top / 12 sides, underline as in Menu.

**Combobox / autocomplete input:** Input with an absolute start icon (16px at 80%, 11px from the edge) and trigger/clear buttons 28x28 (32) radius 6 at 80% opacity.

**Autocomplete popup:** same as combobox but solid: 1px border, bg popover, `shadow-lg/5`, edge line. Item hover and highlighted bg accent, selected bg accent/50.

### 4.26 Label (`label.tsx`)

14px (16) medium foreground, line-height 16 (18), inline-flex gap 8.

## 5. Icons

`lucide-react` ^0.564.0 (`package.json:78`), imported in 350 files, about 290 distinct icons. Rendered as 24-unit viewBox strokes with round caps and joins. **Stroke width is 1.75**, not lucide's 2: a base-layer rule rewrites every `stroke-width="2"` (`index.css:1752-1756`). Explicit overrides exist: toast close and expanders 2.25, checkbox marks 3.

Default sizes:

| context | size |
|---|---|
| button, toggle, menu item, select trigger/item | 16px (18) |
| button `xs`, `compact`, `icon-xs`, select `xs`/`compact`, check in menu `icon` variant | 14px |
| button `micro`, `icon-micro`, `icon-tiny`, kbd, badge (desktop), toast close | 12px |
| button `xl`, `icon-xl` | 18px (20) |
| sidebar rows | `--sidebar-icon-color` (#86858e / #8b8b8b) |

Most used (JSX references): ChevronDown 73, X 65, Check 64, Plus 63, ChevronRight 48, ExternalLink 29, TriangleAlert 25, Copy 22, MessageSquare 22, CircleAlert 22, Folder 20, CircleCheck 19, Download 19, Search 18, Layers 17, Info 14, Terminal 14, Trash2 14, Eye 14, FolderGit2 13, ArrowRight 13, FolderPlus 13, Play 13, GitBranch 11, AlertTriangle 11, Minus 11, ChevronUp 10, Settings 9, Monitor 9, SquarePen 9, CircleDashed 9, Bot 9, LoaderCircle (spinner), RefreshCw.

Not lucide: file-type icons in trees and chips come from `@pierre/trees`' sprite sheet plus a few custom symbols with fixed colors (`pierre-icons.ts`); project favicons and provider logos are images or custom SVGs. Bundle the lucide SVGs you need as GPUI `svg()` assets and stroke them at 1.75.

## 6. Themes and switching

- **Preference** (`hooks/useTheme.ts`, `themePalette.ts:1743-1856`): stored in localStorage `t3code:theme`, default `"system"`. Values: `system`, `light`, `dark`, or a theme id. A separate appearance mode (`light` / `dark` / `system`, key `t3code:theme-appearance-mode`) picks the half of a two-mode theme, and "theme halves" (`t3code:theme-halves:v1`) allow a different theme for light and dark. A single-appearance theme forces its appearance. `system` follows `prefers-color-scheme` live.
- **Applying:** toggle `.dark` on `<html>`, set or clear `data-theme-id`, write the theme's 60 roles as `--app-theme-*` inline variables, suppress transitions for one frame, and tell the Electron shell (`bridge.setTheme`) so native chrome matches.
- **Built-in named themes** (`packages/shared/src/themePalettes.ts:1,238-912`): T3 Chat (pink), Grove (green), Ocean (blue), Ember (orange), Iris (violet). Each has light and dark, turns on sidebar artwork, and tints the toolbar, sidebar, focus color and inline code. Full hex palettes are in Appendix A. They are maintainer palettes, carefully tuned, not filler; they are optional for a pixel match of the default look.
- **Custom themes:** a theme editor with a color picker per role, a guided generator from one accent color (`createVividThemeColors`), an inspector overlay, raw CSS per theme (sanitized so it cannot fetch URLs, `themeCss.ts`), import of VS Code theme JSON (`vscodeThemeImport.ts`), and an **OpenVSX** search that downloads `.vsix` extensions and converts their themes (`openVsxThemes.ts`, network to open-vsx.org). Environments (servers) can publish themes and background images (`themeBackground.ts`). This is a large amount of surface for a niche feature; for Tern I would keep light/dark/system and the five built-ins, and drop the editor, CSS injection, OpenVSX and server-published themes.
- **Role to token mapping** when a named theme is active (`index.css:1358-1424`): canvas to background, chrome to app-chrome-background, surface to card, surfaceOverlay to popover, text to foreground, messageAction to **primary** (so buttons, switches and the send button use the theme's action color), accentSurface to accent, focus to ring, plus one-to-one for the rest. The theme role named `accent` is not mapped to `--accent` and nothing in the main UI paints it (only the import dialog reads `--app-theme-accent`). Info and success stay on the Tailwind defaults in every theme. Under a named theme the sidebar subtree reads the theme's sidebar roles (`index.css:1587-1615`) and the chat header uses the toolbar roles (`index.css:1500-1544`).

## Appendix A. Theme palettes (hex)

Generated from `packages/shared/src/themePalettes.ts` (oklch to sRGB, clipped). `T3_CODE_LIGHT/DARK` is the shared reference palette used by mobile and as the theme editor's starting point. **The desktop default does not paint from it**; it paints from section 1.2, which differs in places (light `text` is #18181b there vs #27272a here, dark canvas #000000 vs #0a0a0a, dark message surface #1e1e1e vs #141414, `STANDARD_WEB_PAINTED_COLORS` in `themePalette.ts:446-449`).

| role | T3_CODE_LIGHT | T3_CODE_DARK | T3_CHAT light | T3_CHAT dark | GROVE light | GROVE dark | OCEAN light | OCEAN dark | EMBER light | EMBER dark | IRIS light | IRIS dark |
|---|---|---|---|---|---|---|---|---|---|---|---|---|
| canvas | #fcfcfc | #0a0a0a | #fdf7fd | #1f1a24 | #f3f7f4 | #1b2821 | #f5f7f8 | #17212b | #f9f7f5 | #291e1a | #f8f7f9 | #1d1929 |
| chrome | #fcfcfc | #0a0a0a | #fdf7fd | #1f1a24 | #f3f7f4 | #1b2821 | #f5f7f8 | #17212b | #f9f7f5 | #291e1a | #f8f7f9 | #1d1929 |
| toolbar | #fcfcfc | #0a0a0a | #fdf7fd | #1f1a24 | #f3f7f4 | #1b2821 | #f5f7f8 | #17212b | #f9f7f5 | #291e1a | #f8f7f9 | #1d1929 |
| toolbarForeground | #27272a | #f5f5f5 | #501854 | #f9f8fb | #241523 | #fffaff | #241523 | #fffaff | #241523 | #fffaff | #241523 | #fffaff |
| toolbarBorder | #e4e4e7 | #191919 | #efbdeb | #27242c | #d5e6dd | #36654c | #d8e4ee | #36566f | #eee0d9 | #6e4934 | #e5e0f0 | #4a3c70 |
| toolbarControl | #ffffff | #111111 | #f3e6f5 | #362d3d | #e2ede7 | #2a4b39 | #e4ecf2 | #293f52 | #f3eae5 | #513728 | #edeaf4 | #362d51 |
| toolbarControlForeground | #27272a | #f5f5f5 | #501854 | #d4c7e1 | #241523 | #fffaff | #241523 | #fffaff | #241523 | #fffaff | #241523 | #fffaff |
| toolbarControlHover | #f4f4f5 | #141414 | #eccfe3 | #463753 | #d5e6dd | #325c46 | #d8e4ee | #324e66 | #eee0d9 | #644330 | #e5e0f0 | #433765 |
| surface | #ffffff | #111111 | #faf3fb | #29232d | #f3f7f4 | #1b2821 | #f5f7f8 | #17212b | #f9f7f5 | #291e1a | #f8f7f9 | #1d1929 |
| surfaceRaised | #fcfcfc | #111111 | #fdfafd | #2c2631 | #ecefed | #36413c | #edeff1 | #333b45 | #f1efee | #433835 | #f0eff2 | #383443 |
| surfaceOverlay | #ffffff | #111111 | #ffffff | #100a0e | #e7e9e8 | #444d49 | #e8e9eb | #414851 | #ece9e9 | #4f4543 | #ebe9ed | #454250 |
| text | #27272a | #f5f5f5 | #501854 | #f9f8fb | #241523 | #fffaff | #241523 | #fffaff | #241523 | #fffaff | #241523 | #fffaff |
| textMuted | #71717b | #818181 | #ac1668 | #e7d0dd | #746c73 | #919595 | #746c75 | #8d8f97 | #766c74 | #968e8f | #766c76 | #8e8a95 |
| border | #e4e4e7 | #191919 | #eee1ed | #27242c | #cbd5d1 | #415f4f | #cdd4dc | #405567 | #ddd2ce | #664c3f | #d6d1de | #4d4366 |
| input | #d4d4d8 | #1e1e1e | #e7c1dc | #302029 | #becbc5 | #4f725f | #c0c9d4 | #4f677b | #d4c6c1 | #7a5d4d | #ccc5d6 | #5d527b |
| focus | #1b4ed8 | #346bf1 | #db2777 | #db2777 | #1b7d50 | #69d69a | #2672af | #70b9ee | #ae552a | #f09a64 | #7253b9 | #9d7df2 |
| accent | #1b4ed8 | #346bf1 | #db2777 | #a3004c | #1b7d50 | #69d69a | #2672af | #70b9ee | #ae552a | #f09a64 | #7253b9 | #9d7df2 |
| accentForeground | #ffffff | #ffffff | #ffffff | #fbd0e8 | #fffaff | #241523 | #fffaff | #241523 | #fffaff | #241523 | #fffaff | #241523 |
| secondary | #fafafa | #111111 | #f1c4e6 | #362d3d | #e2ede7 | #2a4b39 | #e4ecf2 | #293f52 | #f3eae5 | #513728 | #edeaf4 | #362d51 |
| secondaryForeground | #27272a | #f5f5f5 | #77347c | #d4c7e1 | #241523 | #fffaff | #241523 | #fffaff | #241523 | #fffaff | #241523 | #fffaff |
| muted | #fafafa | #111111 | #eaa7cb | #423a45 | #e6f0ea | #253e31 | #e8eff4 | #233544 | #f4ede9 | #432e23 | #f0edf6 | #2d2643 |
| mutedForeground | #71717b | #818181 | #8d1255 | #e7d0dd | #6e696f | #9da5a2 | #6f6873 | #969ca6 | #74686f | #a59996 | #726874 | #9690a1 |
| placeholder | #71717b | #818181 | #8b5f90 | #968d9f | #716971 | #a9abab | #716972 | #a4a4ac | #736971 | #aba3a5 | #736973 | #a29ea8 |
| secondaryLabel | #71717b | #818181 | #ac1668 | #e7d0dd | #746c73 | #919595 | #746c75 | #8d8f97 | #766c74 | #968e8f | #766c76 | #8e8a95 |
| iconMuted | #71717b | #818181 | #ac1668 | #d4c7e1 | #746c73 | #919595 | #746c75 | #8d8f97 | #766c74 | #968e8f | #766c76 | #8e8a95 |
| error | #fb2c36 | #fb414a | #f7086c | #9d174d | #fb2c36 | #fb414a | #fb2c36 | #fb414a | #fb2c36 | #fb414a | #fb2c36 | #fb414a |
| errorForeground | #c10007 | #ff6467 | #9d174d | #fbd0e8 | #c10007 | #ff6668 | #c10007 | #ff6467 | #c10007 | #ff6467 | #c10007 | #ff6467 |
| errorSurface | #fcebec | #301214 | #fde4f1 | #331a2b | #f4e7e5 | #3f2c28 | #f5e6e9 | #3c2630 | #f9e7e6 | #4a2321 | #f8e6ea | #40202e |
| warning | #fe9a00 | #fe9a00 | #f59e0b | #f59e0b | #fe9a00 | #fe9a00 | #fe9a00 | #fe9a00 | #fe9a00 | #fe9a00 | #fe9a00 | #fe9a00 |
| warningForeground | #bb4d00 | #ffb900 | #b05109 | #fbbf24 | #b64a00 | #ffb900 | #b74b00 | #ffb900 | #b84b00 | #ffb900 | #b84b00 | #ffb900 |
| warningSurface | #fcf4e8 | #312108 | #fcf0ea | #412f20 | #f4f0e1 | #3f3a1c | #f6efe4 | #3c3424 | #f9efe2 | #4b3215 | #f8efe5 | #412e23 |
| update | #1b4ed8 | #346bf1 | #db2777 | #a3004c | #1b7d50 | #69d69a | #2672af | #70b9ee | #ae552a | #f09a64 | #7253b9 | #9d7df2 |
| updateForeground | #1b4ed8 | #51a2ff | #ac1668 | #fbd0e8 | #125134 | #9ee4be | #194a72 | #a2d2f4 | #71381b | #f5bd9a | #4a3678 | #bfabf7 |
| updateSurface | #e0e6f7 | #121b34 | #fadfef | #37152b | #d0e3da | #345f48 | #d4e1ed | #345269 | #edddd5 | #684631 | #e2ddef | #463969 |
| accentSurface | #f4f4f5 | #141414 | #f3e6f5 | #463753 | #d5e6dd | #325c46 | #d8e4ee | #324e66 | #eee0d9 | #644330 | #e5e0f0 | #433765 |
| accentSurfaceForeground | #18181b | #f5f5f5 | #454554 | #f8f1f5 | #241523 | #fffaff | #241523 | #fffaff | #241523 | #fffaff | #241523 | #fffaff |
| messageSurface | #f4f4f5 | #141414 | #f7def2 | #2b2431 | #cce1d7 | #37664d | #d0dfeb | #375871 | #ebdad1 | #704b34 | #e0d9ee | #4b3d72 |
| messageForeground | #27272a | #f5f5f5 | #492c61 | #f2ebfa | #241523 | #fffaff | #241523 | #fffaff | #241523 | #fffaff | #241523 | #fffaff |
| messageAction | #1b4ed8 | #346bf1 | #db2777 | #a3004c | #8f6410 | #e3b34e | #0a6f75 | #5bd0d6 | #b23535 | #f78a7a | #a82c87 | #f099d8 |
| messageActionForeground | #ffffff | #ffffff | #ffffff | #fbd0e8 | #fffaff | #241523 | #fffaff | #241523 | #fffaff | #241523 | #fffaff | #241523 |
| messageActionHover | #3160db | #3061d9 | #c12269 | #a2004c | #7e580e | #e6bc63 | #096267 | #6fd6db | #9d2f2f | #f8988a | #942777 | #f2a5dd |
| codeBackground | #ffffff | #111111 | #f5ecf9 | #1f1a24 | #eef1ef | #28342e | #f0f1f3 | #252e38 | #f3f1f0 | #362b27 | #f2f1f4 | #2a2736 |
| codeForeground | #27272a | #f5f5f5 | #673c8b | #d8c3ef | #241523 | #fffaff | #241523 | #fffaff | #241523 | #fffaff | #241523 | #fffaff |
| searchMatchBackground | #ffde8b | #8c6c1f | #ffde8b | #8c6c1f | #ffde8b | #8c6c1f | #ffde8b | #8c6c1f | #ffde8b | #8c6c1f | #ffde8b | #8c6c1f |
| searchMatchForeground | #21180a | #f5f5f5 | #21180a | #f5f5f5 | #21180a | #f5f5f5 | #21180a | #f5f5f5 | #21180a | #f5f5f5 | #21180a | #f5f5f5 |
| searchMatchActiveBackground | #eea52b | #eea52b | #eea52b | #eea52b | #eea52b | #eea52b | #eea52b | #eea52b | #eea52b | #eea52b | #eea52b | #eea52b |
| searchMatchActiveForeground | #21180a | #21180a | #21180a | #21180a | #21180a | #21180a | #21180a | #21180a | #21180a | #21180a | #21180a | #21180a |
| sidebar | #fafafa | #000000 | #f2e1f4 | #171018 | #e2ede7 | #21362b | #e4ecf2 | #1e2d3b | #f3eae5 | #39281f | #edeaf4 | #272139 |
| sidebarForeground | #27272a | #f1f3f7 | #454554 | #f4f4f5 | #241523 | #fffaff | #241523 | #fffaff | #241523 | #fffaff | #241523 | #fffaff |
| sidebarMutedForeground | #71717b | #a3a3a3 | #ac1668 | #e7d0dd | #6b666c | #9da3a2 | #6c6570 | #989ca5 | #71646b | #a49998 | #6f6471 | #9792a0 |
| sidebarControlSurface | #f4f4f5 | #0a0a0a | #f8f8f7 | #261922 | #d3dcd8 | #45554d | #d5dbe2 | #424e5a | #e2d9d6 | #584943 | #ddd9e3 | #494459 |
| sidebarRowHover | #fcfcfc | #131313 | #f8f8f7 | #261922 | #cae0d5 | #2a4938 | #cdddea | #283e50 | #ead8cf | #4f3528 | #ded8ed | #352c4f |
| sidebarRowActive | #ffffff | #1a1b1b | #f8f8f7 | #261922 | #bad7c9 | #2f5641 | #bed4e5 | #2f495f | #e5ccc0 | #5d3f2d | #d4cce8 | #3f345e |
| sidebarRowSelected | #ffffff | #111111 | #f8f8f7 | #261922 | #b2d2c3 | #325c46 | #b7cfe2 | #324f66 | #e2c6b8 | #654330 | #cfc6e6 | #433766 |
| sidebarBorder | #e4e4e7 | #141414 | #eceae9 | #322028 | #cbd3d0 | #6f7a75 | #cdd2d9 | #6d757f | #dad0ce | #7e716e | #d5d0db | #736d7e |
| terminalBackground | #fcfcfc | #0a0a0a | #fdf7fd | #1f1a24 | #f3f7f4 | #1b2821 | #f5f7f8 | #17212b | #f9f7f5 | #291e1a | #f8f7f9 | #1d1929 |
| terminalForeground | #27272a | #f5f5f5 | #501854 | #f9f8fb | #241523 | #fffaff | #241523 | #fffaff | #241523 | #fffaff | #241523 | #fffaff |
| terminalCursor | #26384e | #b4cbff | #db2777 | #db2777 | #1b7d50 | #69d69a | #2672af | #70b9ee | #ae552a | #f09a64 | #7253b9 | #9d7df2 |
| terminalSelection | #d0d6dd | #343a47 | #f1c4e6 | #362d3d | #cce1d7 | #36654c | #d0dfeb | #36566f | #ebdad1 | #6e4934 | #e0d9ee | #4a3c70 |
| terminalScrollbar | #d6d6d6 | #222222 | #e7c1dc | #302029 | #c5c5c6 | #7b807e | #c7c5c9 | #797c84 | #cac5c7 | #837a7a | #c9c5ca | #7c7883 |
| terminalScrollbarHover | #bdbdbd | #363636 | #eaa7cb | #423a45 | #b1afb1 | #989b9b | #b2aeb4 | #9798a0 | #b5afb2 | #9f9798 | #b4aeb5 | #99959f |
