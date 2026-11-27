import type { On } from 'claude-code'
import { expect, mock, test } from 'claude-code/testing'

const BAND = {
  plugin: 'osampo',
  component: 'AbovePrompt',
  props: {
    hasSurvey: false,
    isCollapsed: false,
    isWorking: false,
    maxRows: 20,
    bodyColumns: 100,
    scroll: { offset: 0, bodyRows: 20 },
    view: {},
  },
} as const

const MIN = 60_000
/** Monday 2026-11-23, local time. */
const at = (h: number, m = 0) => new Date(2026, 10, 23, h, m).getTime()

const world = (on: On, now: number, stored: Record<string, unknown> = {}) => {
  const clock = mock.clock(on, { now })
  mock.store(on, stored)
  const sounds: unknown[] = []
  on('command.register', () => ({ value: { command: 'osampo' } }))
  on('session.start', (_, e) => ({ cwd: e.cwd }))
  on('prompt.submit', (_, e) => ({ text: e.text }))
  on('turn.complete', () => ({ text: '' }))
  on('tool.call', () => ({ result: {} }))
  on('ui.toast', () => ({ value: undefined }))
  on('audio.play', (_, e) => (sounds.push(e), { value: undefined }))

  return { clock, sounds }
}

const start = { cwd: '/', surface: 'terminal', isInteractive: true } as const
const turn = { reason: 'answer', answer: 'done', text: 'done', durationMs: 1, isAborted: false, turnId: 't', messages: [] } as any
const run = (args: string) => ({ command: 'osampo', args }) as any

test('paw when Claude waits, bark after 2 minutes, down on the next prompt', async ($, on) => {
  const { clock, sounds } = world(on, at(10))
  await $.session.start(start)
  await $.turn.complete(turn)

  const ui = await $.ui.mount({ ...BAND, surface: 'terminal' })
  expect(await ui.find({ type: 'Text', text: /Claude is waiting/ })).toBeTruthy()

  await clock.advance(2 * MIN + 5000)
  expect(await ui.find({ type: 'Text', text: /WOUF/ })).toBeTruthy()
  expect(sounds.length).toBe(1)

  await $.prompt.submit({ text: 'hi' } as any)
  expect(await ui.find({ type: 'Text', text: /sits beside you/ })).toBeTruthy()
  await ui.unmount()
})

test('90 minutes of work empties the walk gauge', async ($, on) => {
  const { clock } = world(on, at(8))
  await $.session.start(start)

  for (let i = 0; i < 23; i++) {
    await clock.advance(4 * MIN)
    await $.prompt.submit({ text: 'work' } as any)
  }

  const ui = await $.ui.mount({ ...BAND, surface: 'terminal' })
  expect(await ui.find({ type: 'Text', text: /take me for a walk/ })).toBeTruthy()
  await ui.press({ key: 'walk' })
  expect(await ui.find({ type: 'Text', text: /off for a walk/ })).toBeTruthy()
  await ui.unmount()
})

test('the noon reminder asks, snoozes 10 minutes, comes back, is accepted', async ($, on) => {
  const { clock } = world(on, at(12, 1))
  await $.session.start(start)

  for (const surface of ['terminal', 'desktop'] as const) {
    const ui = await $.ui.mount({ ...BAND, surface })
    expect(await ui.find({ type: 'Text', text: /meal time/ })).toBeTruthy()
    await ui.unmount()
  }

  const ui = await $.ui.mount({ ...BAND, surface: 'terminal' })
  await ui.press({ key: 'snooze' })
  expect(await ui.find({ type: 'Text', text: /sits beside you/ })).toBeTruthy()
  await clock.advance(10 * MIN + 5000)
  expect(await ui.find({ type: 'Text', text: /meal time/ })).toBeTruthy()
  await ui.press({ key: 'walk' })
  expect(await ui.find({ type: 'Text', text: /enjoys the meal/ })).toBeTruthy()
  await clock.advance(5 * MIN)
  expect(await ui.find({ type: 'Text', text: /meal time/ })).toBeFalsy()
  await ui.unmount()
})

test('no sound during quiet hours', async ($, on) => {
  const { clock, sounds } = world(on, at(23))
  await $.session.start(start)
  await $.turn.complete(turn)
  await clock.advance(3 * MIN)
  expect(sounds.length).toBe(0)
})

test('/osampo cares, hides, shows and explains itself', async ($, on) => {
  world(on, at(10))
  await $.session.start(start)

  expect((await $.command.run(run('bone'))).text).toMatch(/chews its bone/)
  expect((await $.command.run(run('nope'))).text).toMatch(/^\/osampo walk/)
  await $.command.run(run('hide'))
  const hiddenBand = await $.ui.mount({ ...BAND, surface: 'terminal' })
  expect(await hiddenBand.find({ type: 'Text', text: /Osampo/ })).toBeFalsy()
  await hiddenBand.unmount()
  await $.command.run(run('show'))
  const shownBand = await $.ui.mount({ ...BAND, surface: 'terminal' })
  expect(await shownBand.find({ type: 'Text', text: /Osampo/ })).toBeTruthy()
  await shownBand.unmount()
})

test('a button answers the paw', async ($, on) => {
  world(on, at(10))
  await $.session.start(start)
  await $.turn.complete(turn)
  const ui = await $.ui.mount({ ...BAND, surface: 'terminal' })
  await ui.press({ key: 'pet' })
  expect(await ui.find({ type: 'Text', text: /rolls over/ })).toBeTruthy()
  await ui.unmount()
})

test('one wouf per request, none after a relance or a turn, none when hidden', async ($, on) => {
  const { clock, sounds } = world(on, at(11, 59))
  await $.session.start(start)
  await clock.advance(MIN + 5000)
  expect(sounds.length).toBe(1)
  await clock.advance(30_000 + MIN)
  // wouf, then the 60 s follow-up bark, and no wouf when it ends.
  expect(sounds.length).toBe(2)
  await $.turn.complete(turn)
  await $.prompt.submit({ text: 'hi' } as any)
  expect(sounds.length).toBe(2)
  await $.command.run(run('hide'))
  await clock.advance(10 * MIN)
  expect(sounds.length).toBe(2)
})

test('a replaced reminder does not come back', async ($, on) => {
  const { clock } = world(on, at(12, 1))
  await $.session.start(start)
  await clock.advance(30 * MIN)
  const ui = await $.ui.mount({ ...BAND, surface: 'terminal' })
  expect(await ui.find({ type: 'Text', text: /walk time/ })).toBeTruthy()
  await ui.press({ key: 'walk' })
  await clock.advance(11 * MIN)
  expect(await ui.find({ type: 'Text', text: /meal time/ })).toBeFalsy()
  await ui.unmount()
})

test('an approved command drops the paw while it runs', async ($, on) => {
  world(on, at(10))
  on('classic.PermissionRequest', () => ({}))
  on('tool.run', () => ({ result: {} }))
  await $.session.start(start)
  await $.classic.PermissionRequest({ tool_name: 'Bash', tool_input: {} } as any)
  const ui = await $.ui.mount({ ...BAND, surface: 'terminal' })
  expect(await ui.find({ type: 'Text', text: /Claude is waiting/ })).toBeTruthy()
  await $.tool.run({ tool: 'Bash', command: 'ls' } as any)
  expect(await ui.find({ type: 'Text', text: /Claude is waiting/ })).toBeFalsy()
  await ui.unmount()
})

const PANE = {
  plugin: 'osampo',
  component: 'Pane',
  requestId: 'osampo',
  props: {
    title: 'Osampo',
    isFocused: false,
    bodyColumns: 44,
    placement: 'dock',
    scroll: { offset: 0, bodyRows: 40 },
    view: {},
  },
} as const

test('the pane shows the board scene, gauges and care buttons', async ($, on) => {
  world(on, at(10))
  on('ui.open', () => ({ value: { isPlaced: true } }))
  await $.session.start(start)

  const ui = await $.ui.mount({ ...PANE, surface: 'terminal' })
  expect(await ui.find({ type: 'Raster' })).toBeTruthy()
  expect(await ui.find({ type: 'Text', text: /sits beside you/ })).toBeTruthy()
  expect(await ui.find({ type: 'Text', text: /12:00 meal/ })).toBeTruthy()
  await ui.press({ key: 'brush' })
  expect(await ui.find({ type: 'Text', text: /enjoys bath time/ })).toBeTruthy()
  await ui.unmount()

  const desk = await $.ui.mount({ ...PANE, surface: 'desktop' })
  expect(await desk.find({ type: 'Svg' })).toBeTruthy()
  await desk.unmount()

  // Docked: the band above the prompt steps aside.
  const band = await $.ui.mount({ ...BAND, surface: 'terminal' })
  expect(await band.find({ type: 'Text', text: /Osampo/ })).toBeFalsy()
  await band.unmount()
})

test('the scene animates: the pane is redrawn with the next frame', async ($, on) => {
  const { clock } = world(on, at(10))
  on('ui.open', () => ({ value: { isPlaced: true } }))
  await $.session.start(start)
  const ui = await $.ui.mount({ ...PANE, surface: 'terminal' })
  const first = (await ui.find({ type: 'Raster' })) as any
  const seen = new Set([JSON.stringify(first)])

  for (let i = 0; i < 8; i++) {
    await clock.advance(150)
    seen.add(JSON.stringify(await ui.find({ type: 'Raster' })))
  }

  expect(seen.size).toBeGreaterThanOrEqual(2)
  await ui.unmount()
})

test('two pending permissions: approving one keeps the paw up', async ($, on) => {
  world(on, at(10))
  on('classic.PermissionRequest', () => ({}))
  on('tool.run', () => ({ result: {} }))
  await $.session.start(start)
  await $.classic.PermissionRequest({ tool_name: 'Bash', tool_input: {} } as any)
  await $.classic.PermissionRequest({ tool_name: 'Bash', tool_input: {}, agent_id: 'a1' } as any)
  const ui = await $.ui.mount({ ...BAND, surface: 'terminal' })
  await $.tool.run({ tool: 'Bash', command: 'ls' } as any)
  expect(await ui.find({ type: 'Text', text: /Claude is waiting/ })).toBeTruthy()
  await $.tool.run({ tool: 'Bash', command: 'ls', agentId: 'a1' } as any)
  expect(await ui.find({ type: 'Text', text: /Claude is waiting/ })).toBeFalsy()
  await ui.unmount()
})

test('a snoozed walk request woufs again once the snooze ends', async ($, on) => {
  const { clock, sounds } = world(on, at(8))
  await $.session.start(start)

  for (let i = 0; i < 23; i++) {
    await clock.advance(4 * MIN)
    await $.prompt.submit({ text: 'work' } as any)
  }

  const asked = sounds.length
  expect(asked).toBeGreaterThanOrEqual(1)
  await $.command.run(run('snooze'))
  await clock.advance(11 * MIN)
  expect(sounds.length).toBe(asked + 1)
})

test('a denied subagent request does not hold the paw up later', async ($, on) => {
  world(on, at(10))
  on('classic.PermissionRequest', () => ({}))
  on('tool.run', () => ({ result: {} }))
  await $.session.start(start)
  await $.classic.PermissionRequest({ tool_name: 'Bash', tool_input: {}, agent_id: 'a1' } as any)
  // Denied: the call ends without running.
  await $.tool.call({ tool: 'Bash', command: 'rm -rf x', agentId: 'a1' } as any)
  await $.classic.PermissionRequest({ tool_name: 'Bash', tool_input: {} } as any)
  const ui = await $.ui.mount({ ...BAND, surface: 'terminal' })
  expect(await ui.find({ type: 'Text', text: /Claude is waiting/ })).toBeTruthy()
  await $.tool.run({ tool: 'Bash', command: 'ls' } as any)
  expect(await ui.find({ type: 'Text', text: /Claude is waiting/ })).toBeFalsy()
  await ui.unmount()
})

test("a subagent's own work does not drop the paw while you are awaited", async ($, on) => {
  world(on, at(10))
  await $.session.start(start)
  await $.turn.complete(turn)
  await $.tool.call({ tool: 'Read', file_path: '/x', agentId: 'bg' } as any)
  const ui = await $.ui.mount({ ...BAND, surface: 'terminal' })
  expect(await ui.find({ type: 'Text', text: /Claude is waiting/ })).toBeTruthy()
  await ui.unmount()
})

test('a wide panel gets the board at its own pixel; a narrower one fills its width', async ($, on) => {
  world(on, at(10))
  on('ui.open', () => ({ value: { isPlaced: true } }))
  await $.session.start(start)

  const wide = await $.ui.mount({
    ...PANE,
    surface: 'terminal',
    props: { ...PANE.props, bodyColumns: 90, scroll: { offset: 0, bodyRows: 70 } },
  })
  expect(((await wide.find({ type: 'Raster' })) as any).props.columns).toBe(80)
  await wide.unmount()

  // Wide but short (a 50-row screen): still the board's pixel, sky cropped.
  const short = await $.ui.mount({
    ...PANE,
    surface: 'terminal',
    props: { ...PANE.props, bodyColumns: 89, scroll: { offset: 0, bodyRows: 46 } },
  })
  const cropped = ((await short.find({ type: 'Raster' })) as any).props
  expect(cropped.columns).toBe(80)
  expect(cropped.rows).toBeLessThanOrEqual(46 - 8)
  await short.unmount()

  const narrow = await $.ui.mount({ ...PANE, surface: 'terminal' })
  expect(((await narrow.find({ type: 'Raster' })) as any).props.columns).toBe(44)
  await narrow.press({ key: 'leash' })
  expect(await narrow.find({ type: 'Text', text: /off for a walk/ })).toBeTruthy()
  await narrow.unmount()
})

test('hidden stays hidden in the next session', async ($, on) => {
  // A new session: nothing in its state, only what `/osampo hide` saved earlier.
  world(on, at(10), { hidden: true })
  let opens = 0
  on('ui.open', () => (opens++, { value: { isPlaced: true } }))
  await $.session.start(start)
  expect(opens).toBe(0)
})

test('typing your answer drops the paw before the bark', async ($, on) => {
  const { clock, sounds } = world(on, at(10))
  on('prompt.edit', (_, e) => ({ text: e.text, cursor: e.cursor, start: e.start, end: e.end }))
  await $.session.start(start)
  await $.turn.complete(turn)
  await clock.advance(MIN)
  await ($.prompt as any).edit({ origin: 'key', text: 'ok', cursor: 2, start: 0, end: 2, inputText: 'k' } as any)
  await clock.advance(2 * MIN)
  const ui = await $.ui.mount({ ...BAND, surface: 'terminal' })
  expect(await ui.find({ type: 'Text', text: /Claude is waiting|WOUF/ })).toBeFalsy()
  expect(sounds.length).toBe(0)
  await ui.unmount()
})

test('a walk taken while the walk reminder is snoozed settles it', async ($, on) => {
  const { clock, sounds } = world(on, at(12, 31))
  await $.session.start(start)
  await $.command.run(run('snooze'))
  await clock.advance(5 * MIN)
  await $.command.run(run('walk'))
  await clock.advance(20 * MIN)
  const ui = await $.ui.mount({ ...BAND, surface: 'terminal' })
  expect(await ui.find({ type: 'Text', text: /walk time/ })).toBeFalsy()
  expect(sounds.length).toBe(0)
  await ui.unmount()
})

test('two permission requests of the main loop: approving one keeps the paw up', async ($, on) => {
  world(on, at(10))
  on('classic.PermissionRequest', () => ({}))
  on('tool.run', () => ({ result: {} }))
  await $.session.start(start)
  await $.classic.PermissionRequest({ tool_name: 'Bash', tool_input: {} } as any)
  await $.classic.PermissionRequest({ tool_name: 'Bash', tool_input: {} } as any)
  const ui = await $.ui.mount({ ...BAND, surface: 'terminal' })
  await $.tool.run({ tool: 'Bash', command: 'ls', tool_use_id: 't1' } as any)
  expect(await ui.find({ type: 'Text', text: /Claude is waiting/ })).toBeTruthy()
  await $.tool.run({ tool: 'Bash', command: 'pwd', tool_use_id: 't2' } as any)
  expect(await ui.find({ type: 'Text', text: /Claude is waiting/ })).toBeFalsy()
  await ui.unmount()
})

test('a turn you stopped is not Claude waiting; a scripted run makes no sound', async ($, on) => {
  const { clock, sounds } = world(on, at(10))
  await $.session.start({ ...start, isInteractive: false })
  await $.turn.complete({ ...turn, isAborted: true })
  const ui = await $.ui.mount({ ...BAND, surface: 'terminal' })
  expect(await ui.find({ type: 'Text', text: /Claude is waiting/ })).toBeFalsy()
  await $.turn.complete(turn)
  await clock.advance(2 * MIN + 5000)
  expect(await ui.find({ type: 'Text', text: /WOUF/ })).toBeTruthy()
  expect(sounds.length).toBe(0)
  await ui.unmount()
})

test("an ignored reminder's bark sounds once per follow-up, not on every tick", async ($, on) => {
  const { clock, sounds } = world(on, at(12, 30))
  await $.session.start(start)
  await clock.advance(5000)
  const woufs = sounds.length
  await clock.advance(60_000)
  expect(sounds.length).toBe(woufs + 1)
})

test('a second session.start replaces the heartbeat instead of stacking it', async ($, on) => {
  const { clock } = world(on, at(10))
  let polls = 0
  on('ui.panes', () => (polls++, { value: [] }))
  await $.session.start(start)
  await $.session.start(start)
  polls = 0

  await clock.advance(5000)
  expect(polls).toBe(1)
})
