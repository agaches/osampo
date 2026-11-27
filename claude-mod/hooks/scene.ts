// The board's screen, drawn from its own sprites: background, kennel at night, the puppy.
// Terminal: one cell = two stacked pixels ('▀', foreground top, background bottom).
// At 80 columns it is the board's own pixel (80 x 96); a narrower panel gets it scaled down to fit.
import type { Anim } from '../types'
import { SHEETS } from './sprites.gen'
import type { Sheet, SheetName } from './sprites.gen'

/** Board frame time (widgets/osampo/ui/theme.slint). */
export const FRAME_MS = 150

const FULL = { width: 80, height: 96, dog: 64, dogX: 8, dogY: 31 }
const DIGITS = '0123456789abcdefghijklmnopqrstuvwxyz'

/** The board's width in columns: the most a scene takes. */
export const MAX_COLUMNS = FULL.width

/** A scene `columns` wide keeps the board's proportions; its height is whole cells. */
export const dims = (columns: number) => {
  const rows = Math.round((FULL.height * columns) / FULL.width / 2)

  return { columns, rows, width: columns, height: rows * 2 }
}

const SEQUENCES: Record<Anim, readonly SheetName[]> = {
  repos: ['repos'],
  boude: ['boude'],
  patte: ['patte'],
  aboie: ['aboie'],
  laisse: ['laisse'],
  dort: ['dort'],
  content: ['content'],
  roulade: ['roulade'],
  balle: ['court', 'court', 'balle', 'lache'],
  os: ['os', 'os', 'os_fin'],
  bain: ['bain', 'bain_mousse', 'bain_mousse', 'bain_fin'],
  promenade: ['promenade'],
  tournis: ['tournis'],
}

/** Run-length coded palette indices (tools/gen_sprites.py) to 0xAARRGGBB pixels. */
const decode = (sheet: Sheet, f: string, count: number) => {
  const out = new Uint32Array(count)
  let n = 0

  for (let i = 0; i < f.length; ) {
    if (f[i] === '~') {
      const color = sheet.p[DIGITS.indexOf(f[i + 1]!)]!
      const run = DIGITS.indexOf(f[i + 2]!) * 36 + DIGITS.indexOf(f[i + 3]!)
      out.fill(color, n, n + run)
      n += run
      i += 4
    } else {
      out[n++] = sheet.p[DIGITS.indexOf(f[i]!)]!
      i++
    }
  }

  return out
}

const lum = (argb: number) => ((argb >> 16) & 255) * 0.3 + ((argb >> 8) & 255) * 0.59 + (argb & 255) * 0.11

/** Scales down to `w` x `h`: each block's main color, or its dark detail (outline, eye) when it has one. */
const fit = (px: Uint32Array, sw: number, sh: number, w: number, h: number) => {
  if (w === sw && h === sh) {
    return px
  }

  const out = new Uint32Array(w * h)
  const span = (i: number, n: number, sn: number) => {
    const a = Math.floor((i * sn) / n)

    return [a, Math.max(a + 1, Math.floor(((i + 1) * sn) / n))] as const
  }

  for (let y = 0; y < h; y++) {
    const [y0, y1] = span(y, h, sh)

    for (let x = 0; x < w; x++) {
      const [x0, x1] = span(x, w, sw)
      const counts = new Map<number, number>()
      let dark = px[y0 * sw + x0]!

      for (let yy = y0; yy < y1; yy++) {
        for (let xx = x0; xx < x1; xx++) {
          const p = px[yy * sw + xx]!
          counts.set(p, (counts.get(p) ?? 0) + 1)
          dark = lum(p) < lum(dark) ? p : dark
        }
      }

      const main = [...counts].reduce((a, b) => (b[1] > a[1] ? b : a))[0]
      out[y * w + x] = counts.size > 1 && lum(main) - lum(dark) > 60 ? dark : main
    }
  }

  return out
}

const cache = new Map<string, Uint32Array>()

/** One sprite (or background) frame, decoded once. */
const image = (name: SheetName, i: number) => {
  const key = `${name}:${i}`
  const hit = cache.get(key)

  if (hit) {
    return hit
  }

  const sheet = SHEETS[name] as Sheet
  const isBg = name === 'fond_jour' || name === 'fond_nuit'
  const px = decode(sheet, sheet.f[i]!, isBg ? FULL.width * FULL.height : FULL.dog * FULL.dog)
  cache.set(key, px)

  return px
}

/** Every frame of an animation, in the order played. */
const frames = (anim: Anim) => SEQUENCES[anim].flatMap(name => SHEETS[name].f.map((_, i) => ({ name, i })))

export const frameCount = (anim: Anim) => frames(anim).length

const paint = (out: Uint32Array, width: number, sprite: Uint32Array, size: number, x0: number, y0: number) => {
  for (let y = 0; y < size; y++) {
    for (let x = 0; x < size; x++) {
      const argb = sprite[y * size + x]!
      const a = argb >>> 24

      if (a === 0) {
        continue
      }

      const i = (y0 + y) * width + x0 + x
      const under = out[i]!
      const mix = (shift: number) =>
        Math.round((((argb >> shift) & 255) * a + ((under >> shift) & 255) * (255 - a)) / 255) << shift
      out[i] = a === 255 ? argb & 0xffffff : mix(16) | mix(8) | mix(0)
    }
  }
}

/** The scene's pixels `columns` wide, 0xRRGGBB, row-major. */
export const pixels = (anim: Anim, isNight: boolean, tick: number, columns: number = MAX_COLUMNS) => {
  const out = image(isNight ? 'fond_nuit' : 'fond_jour', 0).map(p => p & 0xffffff)
  const at = (sprite: Uint32Array) => paint(out, FULL.width, sprite, FULL.dog, FULL.dogX, FULL.dogY)

  if (anim === 'dort') {
    at(image('niche', 0))
  }

  const all = frames(anim)
  const { name, i } = all[tick % all.length]!
  at(image(name, i))
  const { width, height } = dims(columns)

  return fit(out, FULL.width, FULL.height, width, height)
}

const B64 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/'

const base64 = (bytes: Uint8Array) => {
  let s = ''

  for (let i = 0; i < bytes.length; i += 3) {
    const n = (bytes[i]! << 16) | ((bytes[i + 1] ?? 0) << 8) | (bytes[i + 2] ?? 0)
    s += B64[(n >> 18) & 63]! + B64[(n >> 12) & 63]!
    s += i + 1 < bytes.length ? B64[(n >> 6) & 63]! : '='
    s += i + 2 < bytes.length ? B64[n & 63]! : '='
  }

  return s
}

/** Rows of sky that can go, keeping the puppy's whole cell (and a row above it). */
export const maxCrop = (columns: number) => Math.max(0, Math.floor((FULL.dogY * columns) / FULL.width / 2) - 1)

/** The Raster's `cells`: [codePoint, foreground, background] per cell, little-endian; `skip` sky rows dropped. */
export const cells = (px: Uint32Array, columns: number, skip = 0) => {
  const { width } = dims(columns)
  const rows = dims(columns).rows - skip
  const words = new Uint32Array(columns * rows * 3)

  for (let r = 0; r < rows; r++) {
    for (let c = 0; c < columns; c++) {
      const i = (r * columns + c) * 3
      words[i] = 0x2580
      words[i + 1] = px[2 * (r + skip) * width + c]!
      words[i + 2] = px[(2 * (r + skip) + 1) * width + c]!
    }
  }

  return base64(new Uint8Array(words.buffer))
}

const hex = (rgb: number) => `#${rgb.toString(16).padStart(6, '0')}`

/** The same scene as an SVG, one rect per run of a color, for the surfaces without Raster. */
export const svg = (px: Uint32Array, columns: number = MAX_COLUMNS) => {
  const { width, height } = dims(columns)
  let rects = ''

  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; ) {
      const color = px[y * width + x]!
      let run = 1

      while (x + run < width && px[y * width + x + run] === color) {
        run++
      }

      rects += `<rect x="${x}" y="${y}" width="${run}" height="1" fill="${hex(color)}"/>`
      x += run
    }
  }

  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 ${width} ${height}" shape-rendering="crispEdges">${rects}</svg>`
}
