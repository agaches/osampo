import { atom, read, update } from 'claude-code'
import type { EngineInterface, Register } from 'claude-code'

import type { Pose, Pup, View } from '../types'
import { accept, active, care, isQuietHours, isSoin, newPup, snooze, tick, view } from './puppy'
import type { Soin } from './puppy'
import { FRAME_MS, MAX_COLUMNS, cells, dims, frameCount, maxCrop, pixels, svg } from './scene'

const shown = atom({ plugin: 'osampo', key: 'view' } as const, null)
const hidden = atom({ plugin: 'osampo', key: 'isHidden' } as const, false)
const waitingSince = atom({ plugin: 'osampo', key: 'waitingSince' } as const, null)
const docked = atom({ plugin: 'osampo', key: 'isDocked' } as const, false)

const PANE = 'osampo'
const SCENE = 'scene'

const TINTS: Partial<Record<Pose, string>> = {
  aboie: '#e5534b',
  patte: '#e0a030',
  laisse: '#e0a030',
  content: '#e86fa0',
}

/** Care buttons as drawn: `[ Pet ]`, one column between. */
const CARE = ['Pet', 'Ball', 'Bone', 'Bath', 'Walk'].map(label => label.length + 4)

/** Rows wrapped by `widths` laid in `columns`, `gap` apart. */
const wrapRows = (widths: number[], columns: number, gap = 1) => {
  let rows = 1
  let used = 0

  for (const w of widths) {
    if (used > 0 && used + gap + w > columns) {
      rows++
      used = 0
    }

    used += (used > 0 ? gap : 0) + w
  }

  return rows
}

/** Rows the panel draws besides the scene: header, bubble, gauges, buttons and, unless tight, gaps and a frame. */
const uiRows = (columns: number, isTight: boolean, line: string, isAsking: boolean) => {
  const bubble = isTight ? Math.ceil(line.length / columns) : Math.ceil(line.length / Math.max(1, columns - 4)) + 2
  const parts = [1, bubble, 2, ...(isAsking ? [1] : []), wrapRows(CARE, columns)]
  const gaps = isTight ? 0 : parts.length

  return parts.reduce((a, b) => a + b, 0) + gaps + 1
}
/** The smallest scene still reads as the puppy. */
const MIN_COLUMNS = 24
const BRAND = '#f0b45a'
const EMPTY = '#3b4048'

const level = (value: number) => (value >= 60 ? '#57ab5a' : value >= 30 ? '#e0a030' : '#e5534b')

/** The animation now on screen, kept here so the 150 ms timer reads no state. */
/** `seenAt`: the tick a terminal last drew the pane; past a few frames none is watching. */
const film = { anim: 'repos' as View['anim'], isNight: false, tick: 0, isOn: false, seenAt: 0 }
const frameNo = atom({ plugin: 'osampo', key: 'frame' } as const, 0)
let reel: { cancel: () => void } | null = null
/** The 5 s heartbeat; a new `session.start` (`/clear`, a resumed session) replaces it. */
let heartbeat: { cancel: () => void } | null = null

type Change = (pup: Pup, now: number) => Pup

/** Scripted runs (`claude -p`, the SDK) stay silent: no sound, no toast. */
let isSilent = false
/** One change at a time in this process, so two never read the same puppy and lose one write. */
let queue: Promise<unknown> = Promise.resolve()
/** A reminder's bark is shared by every session: the first to see it sounds it. */
const BARK_GAP_MS = 15_000

const USAGE = '/osampo walk | snooze | pet | ball | bone | brush | hide | show'

/** Reads the stored puppy, applies `change`, saves, redraws, and sounds once per new request or bark. */
const apply = ($: EngineInterface, change: Change = tick) => {
  const run = queue.then(() => applyNow($, change))
  queue = run.catch(() => undefined)

  return run
}

const applyNow = async ($: EngineInterface, change: Change): Promise<View> => {
  const now = await $.clock.now()
  const stored = (await $.store.get('pup')) as Pup | undefined
  const pup = change(stored ?? newPup(now), now)
  await $.store.set('pup', pup)

  const next: View = view(pup, now, await read($, waitingSince))
  const before = await read($, shown)
  await update($, shown, () => next)

  if (next.anim !== film.anim) {
    film.tick = 0
    film.seenAt = 0
  }

  film.anim = next.anim
  film.isNight = next.isNight

  // Requests and reminder barks are the shared puppy's: announced once, whatever the sessions.
  // A bark because Claude waits is this session's own.
  const heard = ((await $.store.get('heard')) as string | undefined) ?? null
  const isNewAsk = next.pose === 'laisse' && next.ask !== heard
  const isWaitBark = next.pose === 'aboie' && next.rappel === null && before?.pose !== 'aboie'
  const barkAt = Number((await $.store.get('barkAt')) ?? 0)
  const isRappelBark =
    next.pose === 'aboie' && next.rappel !== null && before?.pose !== 'aboie' && now - barkAt > BARK_GAP_MS
  const isNewBark = isWaitBark || isRappelBark

  if (next.sage > 0 && heard?.startsWith('walk') && !isNewAsk) {
    await $.store.delete('heard')
  }

  // A session that cannot be heard (scripted, puppy hidden) leaves the announcement to the others.
  if (isSilent || (await read($, hidden))) {
    return next
  }

  if (isNewAsk) {
    await $.store.set('heard', next.ask)
  }

  if (isRappelBark) {
    await $.store.set('barkAt', now)
  }

  // A session just started, or quiet hours: taken as announced, without a sound.
  if (!before || isQuietHours(now)) {
    return next
  }

  if (isNewBark) {
    $.ui.toast(`Osampo: ${next.line}`)
    await $.audio.play({ asset: 'assets/aboie.wav' }).catch(() => undefined)
  } else if (isNewAsk) {
    await $.audio.play({ asset: 'assets/wouf.wav' }).catch(() => undefined)
  }

  return next
}

/** Claude now waits for you in this session. */
const wait = async ($: EngineInterface) => {
  const now = await $.clock.now()
  await update($, waitingSince, since => since ?? now)
  await apply($)
}

/** Opens the pane beside the chat; the band above the prompt steps aside while it is drawn. */
const openPane = async ($: EngineInterface) => {
  const opened = await $.ui.open({ id: PANE, title: 'Osampo', columns: MAX_COLUMNS + 2 }).catch(() => undefined)
  await update($, docked, () => opened?.isPlaced === true)

  if (opened?.isPlaced) {
    roll($)
  }

  return opened?.isPlaced === true
}

/** Plays the film while the pane is drawn; the timer runs only then. */
const roll = ($: EngineInterface) => {
  film.isOn = true
  reel ??= $.clock.every(FRAME_MS, () => frame($))
}

const stop = () => {
  film.isOn = false
  reel?.cancel()
  reel = null
}

/**
 * One frame of the puppy: the pane reads `frame`, so writing it redraws the pane alone.
 * (A blit is taken but an idle terminal does not repaint it, so the pane is redrawn.)
 */
const frame = async ($: EngineInterface) => {
  if (!film.isOn) {
    return stop()
  }

  // Only a terminal plays the film; a desktop's SVG stays still between changes.
  if (film.tick - film.seenAt > 10) {
    return
  }

  film.tick = (film.tick + 1) % (frameCount(film.anim) * 64)
  await update($, frameNo, () => film.tick)
}

/** You (or Claude) moved on: drop the paw, then apply `change`. */
const resume = async ($: EngineInterface, change: Change = tick) => {
  await update($, waitingSince, () => null)

  return apply($, change)
}

export const register: Register = on => {
  let isWorking = false
  /** Permission requests still open, per asker: 'main' or a subagent's id. */
  const askers = new Map<string, number>()
  /** Calls that were let run: one that ends without running was denied. */
  const ran = new Set<string>()
  let typedAt = 0

  const ask = (who: string) => askers.set(who, (askers.get(who) ?? 0) + 1)

  /** One of `who`'s requests is answered; false when it had none open. */
  const answer = (who: string) => {
    const open = askers.get(who) ?? 0

    if (open <= 1) {
      askers.delete(who)
    } else {
      askers.set(who, open - 1)
    }

    return open > 0
  }

  on('session.start', async ($, e, next) => {
    const result = await next(e)
    isSilent = !e.isInteractive
    // Hidden stays hidden from one session to the next.
    const isHidden = (await $.store.get('hidden')) === true
    await update($, hidden, () => isHidden)

    await $.command.register({
      name: 'osampo',
      description: 'The Osampo puppy: walk, snooze, pet, ball, bone, brush, hide, show',
      argumentHint: '[walk|snooze|pet|ball|bone|brush|hide|show]',
      immediate: true,
    })
    await apply($)

    if (!(await read($, hidden))) {
      await openPane($)
    }

    // While a turn runs you are taken to be at your desk, so it counts as work.
    heartbeat?.cancel()
    heartbeat = $.clock.every(5000, async () => {
      const isBusy = isWorking && (await read($, waitingSince)) === null
      await apply($, isBusy ? active : tick)
      const panes = await $.ui.panes().catch(() => [])
      const isPlaced = panes.some(p => p.id === PANE && p.isPlaced)

      if (isPlaced !== (await read($, docked))) {
        await update($, docked, () => isPlaced)
      }

      if (isPlaced && !reel) {
        roll($)
      } else if (!isPlaced) {
        stop()
      }
    })

    return result
  })

  on('prompt.submit', async ($, e, next) => {
    askers.clear()
    await resume($, active)

    return next(e)
  })

  on('prompt.edit', async ($, e, next) => {
    const box = await next(e)
    const now = await $.clock.now()

    // Typing is answering: the paw drops at once. It is work too; once every 30 s is enough.
    if ((await read($, waitingSince)) !== null && askers.size === 0) {
      typedAt = now
      await resume($, active)
    } else if (now - typedAt >= 30_000) {
      typedAt = now
      await apply($, active)
    }

    return box
  })

  on('turn.start', async ($, e, next) => {
    if (!e.agentId) {
      isWorking = true
    }

    return next(e)
  })

  on('tool.call', async ($, e, next) => {
    // A call that ended, run or denied, no longer waits for a permission.
    // A subagent's call settles only its own request; any other call of it says nothing of you.
    const settle = async () => {
      const id = e.tool_use_id
      // Let run: tool.run already answered its request. Never run: denied, its request answered here.
      const wasRun = id !== undefined && ran.delete(id)
      const wasAsking = !wasRun && answer(e.agentId ?? 'main')

      if ((wasAsking || !e.agentId) && askers.size === 0) {
        await resume($, active)
      }
    }

    // A subagent working says nothing about whether the main loop still waits.
    if (e.agentId) {
      const result = await next(e)
      await settle()

      return result
    }

    if (e.tool === 'AskUserQuestion') {
      await wait($)
    } else if (askers.size === 0) {
      await resume($)
    }

    const result = await next(e)
    await settle()

    return result
  })

  // Fires once a call is allowed: an approved command is running, nobody is waited for.
  on('tool.run', async ($, e, next) => {
    if (e.tool !== 'AskUserQuestion') {
      if (e.tool_use_id !== undefined) {
        ran.add(e.tool_use_id)
      }

      if (answer(e.agentId ?? 'main') && askers.size === 0) {
        await resume($, active)
      }
    }

    return next(e)
  })

  on('classic.PermissionRequest', async ($, e, next) => {
    ask(e.agent_id ?? 'main')
    await wait($)

    return next(e)
  })

  on('classic.Notification', async ($, e, next) => {
    await wait($)

    return next(e)
  })

  on('turn.complete', async ($, e, next) => {
    if (!e.agentId) {
      isWorking = false
      askers.clear()
      ran.clear()
      await apply($, active)

      // A turn you stopped yourself is not Claude waiting for you.
      if (!e.isAborted) {
        await wait($)
      }
    }

    return next(e)
  })

  on('ui.close', async ($, e, next) => {
    if (e.id === PANE) {
      stop()
      await update($, docked, () => false)
    }

    return next(e)
  })

  on('command.run', { command: 'osampo' }, async ($, e) => {
    const word = e.args.trim().toLowerCase()

    if (word === 'hide') {
      await update($, hidden, () => true)
      await $.store.set('hidden', true)
      await $.ui.close({ id: PANE }).catch(() => undefined)

      return { text: 'The puppy goes to its kennel.' }
    }

    if (word === 'show' || word === '') {
      await update($, hidden, () => false)
      await $.store.delete('hidden')
      await openPane($)
    }

    if (word === 'show') {
      return { text: 'The puppy is back.' }
    }

    // Typing a command is an answer: Claude is no longer waiting unseen.
    const change: Change | null =
      word === 'walk'
        ? accept
        : word === 'snooze'
          ? snooze
          : isSoin(word)
            ? (pup, now) => care(pup, now, word)
            : word === ''
              ? tick
              : null

    if (!change) {
      return { text: USAGE }
    }

    const puppy = await resume($, change)

    return { text: `Osampo ${puppy.line} · walk ${puppy.sage}% · play ${puppy.jeu}%\n${USAGE}` }
  })

  // The board, beside the chat: its own sprites, animated at its own pace.
  on('ui.render', { component: 'Pane', requestId: PANE }, async ($, e) => {
    const puppy = await read($, shown)
    const { Box, Button, Text } = $.ui.resolve(e)

    if (puppy === null) {
      return <Text dimColor>The puppy is waking up…</Text>
    }

    if (e.surface === 'terminal') {
      film.seenAt = film.tick
    }

    // As wide as the panel allows, up to the board's own pixel; sky cropped, then a tighter
    // layout, then a smaller scene when the panel is short. Bubble, gauges and buttons always fit.
    const isAsking = puppy.rappel !== null || puppy.pose === 'laisse'
    const body = e.props.scroll?.bodyRows ?? 0
    const needs = (c: number) => dims(c).rows - maxCrop(c)
    const ui = (c: number, isTight: boolean) => uiRows(c, isTight, puppy.line, isAsking)
    let columns = Math.min(MAX_COLUMNS, e.props.bodyColumns)
    const narrowest = Math.min(MIN_COLUMNS, columns)

    while (columns > narrowest && body - ui(columns, true) < needs(columns)) {
      columns--
    }

    const isCompact = body - ui(columns, false) < needs(columns)
    const room = body - ui(columns, isCompact)
    const skip = Math.min(maxCrop(columns), Math.max(0, dims(columns).rows - room))
    // Only a terminal plays the film: a desktop keeps one still frame, not redrawn every tick.
    const tick = e.surface === 'terminal' ? await read($, frameNo) : 0
    let picture

    if (e.surface === 'terminal') {
      const { Raster } = $.ui.resolve(e)
      const px = pixels(puppy.anim, puppy.isNight, tick, columns)
      picture = <Raster key={SCENE} columns={columns} rows={dims(columns).rows - skip} cells={cells(px, columns, skip)} />
    } else {
      const { Svg } = $.ui.resolve(e)
      picture = <Svg source={svg(pixels(puppy.anim, puppy.isNight, tick))} alt={`Osampo ${puppy.line}`} width={240} height={288} />
    }

    const soin = (s: Soin) => async () => void (await resume($, (p, now) => care(p, now, s)))
    const cellsWide = Math.max(4, Math.min(30, columns - 12))
    const gauge = (label: string, value: number) => {
      const full = Math.round((value / 100) * cellsWide)

      return (
        <Text>
          <Text dimColor>{label.padEnd(6)}</Text>
          <Text color={level(value)}>{'█'.repeat(full)}</Text>
          <Text color={EMPTY}>{'█'.repeat(cellsWide - full)}</Text>
          <Text dimColor> {String(value).padStart(3)}%</Text>
        </Text>
      )
    }

    return (
      <Box flexDirection="column" gap={isCompact ? 0 : 1} width={columns}>
        <Box justifyContent="space-between">
          <Text bold color={BRAND}>
            OSAMPO
          </Text>
          {puppy.next !== null && <Text dimColor>next {puppy.next}</Text>}
        </Box>
        {picture}
        <Box {...(isCompact ? {} : { borderStyle: 'round', borderColor: TINTS[puppy.pose] ?? EMPTY, paddingX: 1 })}>
          <Text bold color={TINTS[puppy.pose]}>
            {puppy.line}
          </Text>
        </Box>
        <Box flexDirection="column">
          {gauge('Walk', puppy.sage)}
          {gauge('Play', puppy.jeu)}
        </Box>
        {isAsking && (
          <Box gap={1}>
            <Button key="walk" label="Let's go" hotkey="y" onPress={async () => void (await resume($, accept))} />
            <Button key="snooze" label="Later" hotkey="l" onPress={async () => void (await resume($, snooze))} />
          </Box>
        )}
        <Box columnGap={1} flexWrap="wrap">
          <Button key="pet" label="Pet" hotkey="1" onPress={soin('pet')} />
          <Button key="ball" label="Ball" hotkey="2" onPress={soin('ball')} />
          <Button key="bone" label="Bone" hotkey="3" onPress={soin('bone')} />
          <Button key="brush" label="Bath" hotkey="4" onPress={soin('brush')} />
          {!isAsking && <Button key="leash" label="Walk" hotkey="w" onPress={async () => void (await resume($, accept))} />}
        </Box>
      </Box>
    )
  })

  // Narrow terminal, no pane: one quiet line above the prompt instead.
  on('ui.render', { component: 'AbovePrompt' }, async ($, e, next) => {
    const puppy = await read($, shown)

    if (e.props.hasSurvey || puppy === null || (await read($, hidden)) || (await read($, docked))) {
      return next(e)
    }

    const { Box, Button, Text } = $.ui.resolve(e)
    const isAsking = puppy.rappel !== null || puppy.pose === 'laisse'

    return (
      <Box gap={1}>
        <Text bold color={TINTS[puppy.pose]}>
          Osampo {puppy.line}
        </Text>
        <Text dimColor>
          · walk {puppy.sage}% · play {puppy.jeu}%
        </Text>
        {isAsking && <Button key="walk" label="Yes" onPress={async () => void (await resume($, accept))} />}
        {isAsking && <Button key="snooze" label="Later" onPress={async () => void (await resume($, snooze))} />}
        <Button key="pet" label="Pet" onPress={async () => void (await resume($, (p, now) => care(p, now, 'pet')))} />
        <Button key="ball" label="Ball" onPress={async () => void (await resume($, (p, now) => care(p, now, 'ball')))} />
      </Box>
    )
  })
}
