/**
 * Purge Success — Equivalence Cards
 *
 * Two tiers:
 *   Tier 1 (< 250 GB): "cloud cost → real-world treat" — no dollar amounts shown
 *   Tier 2 (≥ 250 GB): "Mac SSD upgrade cost" — shows Apple pricing
 *
 * Selection logic randomizes within bracket and avoids consecutive repeats.
 */

export interface EquivalenceCard {
  emoji: string;      // system emoji for treats, "ssd" sentinel for milestones
  title: string;
  description: string;
  isMilestone: boolean;
}

/* ── Card pools ────────────────────────────────────────── */

const TINY: EquivalenceCard[] = [
  { emoji: "☕", title: "That's a coffee, saved", description: "Storage freed that would've cost you a coffee in cloud fees.", isMilestone: false },
  { emoji: "🍪", title: "That's a cookie, saved", description: "A small win — but every byte counts.", isMilestone: false },
  { emoji: "🎵", title: "That's a song download, saved", description: "Enough space back for a few more tracks in your library.", isMilestone: false },
];

const SMALL: EquivalenceCard[] = [
  { emoji: "🧁", title: "That's a cupcake, saved", description: "A sweet little cleanup. Cloud storage you won't need to pay for.", isMilestone: false },
  { emoji: "🍩", title: "That's a donut, saved", description: "Quick and satisfying — just like a good donut.", isMilestone: false },
  { emoji: "🎶", title: "That's a month of music, saved", description: "About what a music streaming month costs in cloud storage terms.", isMilestone: false },
];

const MEDIUM: EquivalenceCard[] = [
  { emoji: "🍔", title: "That's a McMeal, saved", description: "Instead of paying for cloud storage, grab a burger. You've earned it.", isMilestone: false },
  { emoji: "🍕", title: "That's a pizza, saved", description: "Enough storage freed to trade for a whole pizza.", isMilestone: false },
  { emoji: "🧋", title: "That's a boba tea, saved", description: "Treat yourself — this cleanup just paid for one.", isMilestone: false },
  { emoji: "🎮", title: "That's a game rental, saved", description: "One less cloud bill, one more game night.", isMilestone: false },
];

const LARGE: EquivalenceCard[] = [
  { emoji: "🎬", title: "That's a movie ticket, saved", description: "Skip the storage bill, catch a movie instead.", isMilestone: false },
  { emoji: "📚", title: "That's a book, saved", description: "Enough saved for a good paperback — or more disk space.", isMilestone: false },
  { emoji: "🎧", title: "That's a month of streaming, saved", description: "A full streaming subscription's worth of cloud storage, freed.", isMilestone: false },
  { emoji: "🌮", title: "That's a nice lunch out, saved", description: "Cloud savings big enough to buy lunch. Not bad for a cleanup.", isMilestone: false },
];

const BIG: EquivalenceCard[] = [
  { emoji: "🍽️", title: "That's a nice dinner out, saved", description: "Cloud bills this big could've been a proper meal. Now it's yours.", isMilestone: false },
  { emoji: "👟", title: "That's a pair of sneakers, saved", description: "Enough storage costs to buy shoes. Your drive's lighter too.", isMilestone: false },
  { emoji: "🎫", title: "That's a concert ticket, saved", description: "From cloud fees to live music — that's a worthy trade.", isMilestone: false },
  { emoji: "🕹️", title: "That's a new game, saved", description: "Full price game's worth of cloud storage, back on your machine.", isMilestone: false },
];

const MILESTONE_250: EquivalenceCard[] = [
  { emoji: "ssd", title: "Worth a $100 Mac storage upgrade", description: "Apple charges ~$100 extra for 256 GB more when you buy a Mac. You just freed that — for nothing.", isMilestone: true },
  { emoji: "ssd", title: "Like getting a free storage bump", description: "That's an entire storage tier on a MacBook, reclaimed without spending a cent.", isMilestone: true },
];

const MILESTONE_512: EquivalenceCard[] = [
  { emoji: "ssd", title: "Worth a $200 Mac storage upgrade", description: "Going from 512 GB to 1 TB on a MacBook costs $200. You just freed half a terabyte for free.", isMilestone: true },
  { emoji: "ssd", title: "Half a terabyte — reclaimed", description: "That's an entire base MacBook Air's worth of storage, freed up.", isMilestone: true },
];

const MILESTONE_1TB: EquivalenceCard[] = [
  { emoji: "ssd", title: "Worth a $400 Mac storage upgrade", description: "Going from 1 TB to 2 TB on a MacBook Pro costs $400. You reclaimed a full terabyte — for free.", isMilestone: true },
  { emoji: "ssd", title: "A terabyte. For free.", description: "That's the kind of upgrade people pay hundreds for at checkout. You just got it back.", isMilestone: true },
];

const MILESTONE_2TB: EquivalenceCard[] = [
  { emoji: "ssd", title: "Worth a $600 Mac storage upgrade", description: "The jump from 2 TB to 4 TB costs $600 on a MacBook Pro. You freed two terabytes without spending a thing.", isMilestone: true },
  { emoji: "ssd", title: "Two terabytes. Seriously.", description: "Most people never free this much in a lifetime of cleanups. You just did it in one.", isMilestone: true },
];

/* ── Repeat avoidance ── */

const LAST_CARD_KEY = "kyra_purge_last_card";

function getLastCardTitle(): string | null {
  try { return localStorage.getItem(LAST_CARD_KEY); } catch { return null; }
}

function setLastCardTitle(title: string) {
  try { localStorage.setItem(LAST_CARD_KEY, title); } catch { /* noop */ }
}

/** Pick a random card from pool, avoiding the last shown card */
function randomFromPool(pool: EquivalenceCard[]): EquivalenceCard {
  const last = getLastCardTitle();
  const candidates = pool.length > 1 ? pool.filter((c) => c.title !== last) : pool;
  const pick = candidates[Math.floor(Math.random() * candidates.length)];
  setLastCardTitle(pick.title);
  return pick;
}

/* ── Public API ─────────────────────────────────────────── */

export function pickEquivalenceCard(freedBytes: number): EquivalenceCard {
  const freedGB = freedBytes / (1024 * 1024 * 1024);

  // Tier 2: Milestones (largest first)
  if (freedGB >= 2048) return randomFromPool(MILESTONE_2TB);
  if (freedGB >= 1024) return randomFromPool(MILESTONE_1TB);
  if (freedGB >= 512) return randomFromPool(MILESTONE_512);
  if (freedGB >= 250) return randomFromPool(MILESTONE_250);

  // Tier 1: Treats
  if (freedGB >= 50) return randomFromPool(BIG);
  if (freedGB >= 10) return randomFromPool(LARGE);
  if (freedGB >= 2) return randomFromPool(MEDIUM);
  if (freedGB >= 0.5) return randomFromPool(SMALL);
  return randomFromPool(TINY);
}
