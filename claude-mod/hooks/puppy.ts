// Pure puppy rules, same defaults as widgets/osampo/config.toml. Time is passed in (epoch ms).
import type { Anim, Categorie, Pose, Pup, View } from '../types'

const MIN = 60_000

const REGLES = {
  videApresMin: 90,
  courteMin: 5,
  longueMin: 30,
  reportMin: 10,
  rattrapageMin: 60,
  relancesS: [60, 180, 420],
  calmeApresMin: 10,
  patteAvantAboieMin: 2,
  baisseParJour: 15,
  gain: 20,
  nuit: [20, 7],
  calme: [22, 7],
} as const

/** `jours`: 0 = Sunday, as Date.getDay. */
const RAPPELS: { categorie: Categorie; h: number; m: number; jours?: number[] }[] = [
  { categorie: 'repas', h: 12, m: 0 },
  { categorie: 'sortie', h: 12, m: 30 },
  { categorie: 'sport', h: 18, m: 0, jours: [1, 3, 5] },
  { categorie: 'repas', h: 19, m: 30 },
  { categorie: 'dodo', h: 21, m: 45 },
]

const ASKS: Record<Categorie, string> = {
  repas: 'meal time!',
  sortie: 'walk time! brings the leash',
  sport: 'sport time! brings the leash',
  dodo: 'last walk, then bed',
}

export type Soin = 'pet' | 'ball' | 'bone' | 'brush'

const SOINS: Record<Soin, { text: string; anim: Anim }> = {
  pet: { text: 'rolls over', anim: 'roulade' },
  ball: { text: 'fetches the ball', anim: 'balle' },
  bone: { text: 'chews its bone', anim: 'os' },
  brush: { text: 'enjoys bath time', anim: 'bain' },
}

const LABELS: Record<Categorie, string> = { repas: 'meal', sortie: 'walk', sport: 'sport', dodo: 'last walk' }

const hhmm = (h: number, m: number) => `${String(h).padStart(2, '0')}:${String(m).padStart(2, '0')}`

/** The next reminder still to come today. */
export const nextRappel = (now: number) => {
  const day = new Date(now)
  const r = RAPPELS.filter(r => !r.jours || r.jours.includes(day.getDay())).find(
    r => new Date(day.getFullYear(), day.getMonth(), day.getDate(), r.h, r.m).getTime() > now,
  )

  return r ? `${hhmm(r.h, r.m)} ${LABELS[r.categorie]}` : null
}

export const isSoin = (word: string): word is Soin => Object.hasOwn(SOINS, word)

const clamp = (x: number) => Math.min(100, Math.max(0, x))

const inHours = (now: number, [from, to]: readonly [number, number]) => {
  const h = new Date(now).getHours()

  return from > to ? h >= from || h < to : h >= from && h < to
}

export const isQuietHours = (now: number) => inHours(now, REGLES.calme)

export const newPup = (now: number): Pup => ({
  sage: 100,
  jeu: 100,
  lastActive: now,
  lastTick: now,
  rappel: null,
  done: [],
  calmUntil: 0,
  reaction: null,
})

/** The reminders whose catch-up window holds `now` and that were not handled, oldest first. */
const dueRappels = (now: number, done: string[]) => {
  const day = new Date(now)

  return RAPPELS.filter(r => !r.jours || r.jours.includes(day.getDay()))
    .map(r => {
      const dueAt = new Date(day.getFullYear(), day.getMonth(), day.getDate(), r.h, r.m).getTime()

      return { id: `${r.categorie}@${dueAt}`, categorie: r.categorie, dueAt, snoozeUntil: 0 }
    })
    .filter(r => now >= r.dueAt && now - r.dueAt < REGLES.rattrapageMin * MIN && !done.includes(r.id))
}

/** Moves time on: play need drops, a long absence recharges, reminders fire. */
export const tick = (pup: Pup, now: number): Pup => {
  const since = Math.max(0, now - pup.lastTick)
  const jeu = clamp(pup.jeu - (REGLES.baisseParJour * since) / (24 * 60 * MIN))
  const due = dueRappels(now, pup.done)
  const held = pup.rappel && now - pup.rappel.dueAt < REGLES.rattrapageMin * MIN ? pup.rappel : null
  const latest = due.at(-1)
  const rappel = latest && (!held || latest.dueAt > held.dueAt) ? latest : held
  // A newer reminder replaces the older ones: they are missed for good.
  const missed = [...due, ...(held ? [held] : [])]
    .map(r => r.id)
    .filter(id => id !== rappel?.id && !pup.done.includes(id))
  const done = [...pup.done, ...new Set(missed)].slice(-20)

  return { ...pup, jeu, rappel, done, lastTick: Math.max(now, pup.lastTick) }
}

/** You did something: the gap since last time was either work or an absence. */
export const active = (pup: Pup, now: number): Pup => {
  const gap = Math.max(0, now - pup.lastActive)
  const isAbsence = gap >= REGLES.courteMin * MIN
  const sage = isAbsence
    ? clamp(pup.sage + (100 * gap) / (REGLES.longueMin * MIN))
    : clamp(pup.sage - (100 * gap) / (REGLES.videApresMin * MIN))

  return { ...tick(pup, now), sage, lastActive: Math.max(now, pup.lastActive) }
}

export const accept = (pup: Pup, now: number): Pup => {
  // A walk taken while a walk reminder is snoozed settles it too: it will not ask again.
  const asked = asking(pup, now) ?? (pup.rappel?.categorie === 'repas' ? null : pup.rappel)
  const isWalk = asked?.categorie !== 'repas'

  return {
    ...pup,
    sage: isWalk ? 100 : pup.sage,
    done: asked ? [...pup.done, asked.id].slice(-20) : pup.done,
    rappel: asked ? null : pup.rappel,
    calmUntil: now + REGLES.calmeApresMin * MIN,
    reaction: isWalk
      ? { text: 'off for a walk!', until: now + MIN, anim: 'promenade' }
      : { text: 'enjoys the meal', until: now + MIN, anim: 'content' },
  }
}

export const snooze = (pup: Pup, now: number): Pup => {
  const asked = asking(pup, now)

  return asked
    ? { ...pup, rappel: { ...asked, snoozeUntil: now + REGLES.reportMin * MIN } }
    : { ...pup, calmUntil: now + REGLES.reportMin * MIN }
}

export const care = (pup: Pup, now: number, soin: Soin): Pup => ({
  ...pup,
  jeu: clamp(pup.jeu + REGLES.gain),
  reaction: { ...SOINS[soin], until: now + MIN / 2 },
})

/** The reminder asking now: not snoozed, not within the calm after a yes. */
const asking = (pup: Pup, now: number) =>
  pup.rappel && now >= pup.rappel.snoozeUntil && now >= pup.calmUntil ? pup.rappel : null

/** A bark lasts a few seconds at each follow-up delay after a reminder. */
const isRelance = (dueAt: number, now: number) =>
  REGLES.relancesS.some(s => now - dueAt >= s * 1000 && now - dueAt < (s + 10) * 1000)

const look = (pup: Pup, now: number, waitingSince: number | null): { pose: Pose; anim: Anim; line: string } => {
  const rappel = asking(pup, now)

  if (waitingSince !== null) {
    return now - waitingSince >= REGLES.patteAvantAboieMin * MIN
      ? { pose: 'aboie', anim: 'aboie', line: 'WOUF! Claude has been waiting for you' }
      : { pose: 'patte', anim: 'patte', line: 'Claude is waiting for you' }
  }

  if (rappel) {
    const since = Math.max(rappel.dueAt, rappel.snoozeUntil)
    const pose = isRelance(since, now) ? 'aboie' : 'laisse'

    return { pose, anim: pose, line: ASKS[rappel.categorie] }
  }

  if (pup.reaction && now < pup.reaction.until) {
    return { pose: 'content', anim: pup.reaction.anim ?? 'content', line: pup.reaction.text }
  }

  if (pup.sage <= 0 && now >= pup.calmUntil) {
    return { pose: 'laisse', anim: 'laisse', line: 'too long sitting: take me for a walk' }
  }

  if (inHours(now, REGLES.nuit) && now - pup.lastActive >= REGLES.courteMin * MIN) {
    return { pose: 'dort', anim: 'dort', line: 'sleeps' }
  }

  return pup.jeu < 30
    ? { pose: 'assis', anim: 'boude', line: 'wants to play' }
    : { pose: 'assis', anim: 'repos', line: 'sits beside you' }
}

export const view = (pup: Pup, now: number, waitingSince: number | null): View => {
  const rappel = asking(pup, now)
  const isEmpty = pup.sage <= 0 && now >= pup.calmUntil

  return {
    ...look(pup, now, waitingSince),
    isNight: inHours(now, REGLES.nuit),
    next: nextRappel(now),
    sage: Math.round(pup.sage),
    jeu: Math.round(pup.jeu),
    rappel: rappel?.categorie ?? null,
    ask: rappel ? `${rappel.id}:${rappel.snoozeUntil}` : isEmpty ? `walk:${pup.calmUntil}` : null,
  }
}
