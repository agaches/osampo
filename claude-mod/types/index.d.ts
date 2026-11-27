export type Categorie = 'repas' | 'sortie' | 'sport' | 'dodo'

export type Rappel = {
  id: string
  categorie: Categorie
  dueAt: number
  /** Hidden until this time after a snooze. */
  snoozeUntil: number
}

export type Pup = {
  /** Walk gauge, 100 = rested, 0 = asks for its walk. */
  sage: number
  /** Play need, drops over the day, care fills it. */
  jeu: number
  /** Last time you did something in Claude Code. */
  lastActive: number
  lastTick: number
  rappel: Rappel | null
  /** Reminder ids already accepted or missed, newest last. */
  done: string[]
  calmUntil: number
  /** A care reaction on show until `until`. */
  reaction: { text: string; until: number; anim?: Anim } | null
}

/** An animation of the board, one or more sprite sheets played in a row. */
export type Anim =
  | 'repos' | 'boude' | 'patte' | 'aboie' | 'laisse' | 'dort' | 'content'
  | 'roulade' | 'balle' | 'os' | 'bain' | 'promenade' | 'tournis'

export type Pose = 'assis' | 'patte' | 'aboie' | 'laisse' | 'dort' | 'content'

export type View = {
  pose: Pose
  anim: Anim
  isNight: boolean
  line: string
  /** The next reminder today, e.g. "19:30 meal". */
  next: string | null
  sage: number
  jeu: number
  rappel: Categorie | null
  /** Identifies the request to take a walk or eat, so it is announced once. */
  ask: string | null
}

declare module 'claude-code' {
  interface PluginState {
    osampo: {
      view: View | null
      isHidden: boolean
      /** Per session: since when Claude waits for you here. */
      waitingSince: number | null
      /** The pane is drawn beside the chat: the band above the prompt steps aside. */
      isDocked: boolean
      /** The film's frame, written every 150 ms while the pane is drawn. */
      frame: number
    }
  }
}
