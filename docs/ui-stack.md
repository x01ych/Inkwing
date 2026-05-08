# UI stack

The frontend uses **Tailwind CSS v3 + shadcn/ui**.

## Components

shadcn components live in `src/components/ui/` — they are *source*
files that belong to the project, not a runtime library. Edit them in
place when you need a project-specific tweak.

To add a new shadcn primitive:

```bash
pnpm dlx shadcn@latest add <name>     # e.g. accordion, slider, hover-card

# IMPORTANT: shadcn CLI emits "@/lib/utils" / "@/components/ui/..."
# imports. The Vite `@`-alias has been flaky on Windows; the project
# uses relative imports throughout. Convert after every shadcn add:
sed -i 's|from "@/lib/utils"|from "../../lib/utils"|g; \
       s|from "@/components/ui/|from "./|g' src/components/ui/<name>.tsx

# (Linux/macOS sed; on Windows use Git Bash or rewrite by hand.)
```

Re-run after pulls if a teammate adds something.

## Styling helper

`src/lib/utils.ts` exports `cn(...)` (clsx + tailwind-merge). Always
use it for conditional class merging in shadcn-style components:

```tsx
import { cn } from '../../lib/utils';
<div className={cn('px-2', isActive && 'bg-accent')} />
```

## Theme tokens

All colours live in `src/styles/globals.css` as CSS custom properties.
The app is dark-only (`<html class="dark">` in `index.html`). Both
`:root` (light) and `.dark` are defined for completeness, but only
the `.dark` block is rendered.

| Token | Use |
|---|---|
| `--background` / `--foreground` | page-level surface and text |
| `--card` / `--card-foreground` | Card backgrounds |
| `--popover` / `--popover-foreground` | floating layers (Popover/Dialog/Tooltip) |
| `--primary` / `--primary-foreground` | primary buttons, active markers (white-on-black in dark mode) |
| `--secondary` | subtle button / Badge variant |
| `--muted` / `--muted-foreground` | de-emphasised text and surfaces |
| `--accent` / `--accent-foreground` | hover surfaces, sidebar item background |
| `--destructive` | error / danger button + Badge variant |
| `--border` / `--input` / `--ring` | strokes & focus rings |
| `--sidebar*` | dedicated tokens for the navigation sidebar |
| `--chart-1` … `--chart-5` | recharts series colours |

Don't hardcode hex / oklch in components — always go through tokens
(`bg-card`, `text-muted-foreground`, etc.) so the colour story stays
coherent and a future light mode can flip in one place.

## Aesthetic

Pure black-and-white / grey-scale (zinc base). The only non-neutral
colour is `--destructive` (red).

When you find yourself reaching for a colour to convey meaning
(success / warning / info), prefer:
- `Badge` variants (`default` = white-on-black, `secondary`, `outline`)
- `Alert` variants (`default`, `destructive`)
- text weight + position rather than hue

## AntD → shadcn mapping (historical)

The codebase started on Ant Design before migrating to shadcn. If
you're porting more components from elsewhere, this cheat-sheet is the
reference:

| AntD | shadcn |
|---|---|
| `Button` | `Button` |
| `Input` / `InputNumber` | `Input` (use `type="number"`) |
| `Select` (single) | `Select` |
| `Select` (multiple) | `Popover` + `Checkbox` list |
| `Card` | `Card` |
| `Modal` | `Dialog` |
| `Modal.confirm` (destructive) | `AlertDialog` |
| `Drawer` | `Sheet` |
| `Dropdown` | `DropdownMenu` |
| `Tooltip` | `Tooltip` (needs `TooltipProvider` ancestor) |
| `Tabs` | `Tabs` |
| `Switch` | `Switch` |
| `Tag` | `Badge` |
| `Alert` | `Alert` (variant: default / destructive) |
| `Segmented` | `ToggleGroup type="single"` |
| `Spin` | `<Loader2 className="animate-spin" />` (lucide) |
| `Empty` | plain text + Tailwind |
| `Statistic` | bespoke `<Stat>` (label + tabular-nums value) |
| `Descriptions` | `<dl>` + grid |
| `message.success/error` | `toast.success/error` from `sonner` |
| `Form` (rhf-driven) | `Form` from shadcn (which wraps rhf) |

## Icons

`lucide-react`. Browse: <https://lucide.dev>. Pick the closest match
to the existing AntD icon when migrating; for novel buttons prefer
common ones (Plus, Pencil, Trash2, RefreshCcw, Save, Search, X).
