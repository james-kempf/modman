// SPDX-License-Identifier: GPL-3.0-or-later
// Demo Nexus listings for Browse: a fixed catalogue, filtered by period and
// words, sorted and paged like the backend (24 per page).

import type { BrowseRequest, BrowseView, CollectionHit, SearchHit } from "../ipc/types";

const DAY = 86_400_000;
const PAGE = 24;
const PERIOD_DAYS = { all: null, year: 365, month: 30, week: 7, day: 1 } as const;

const ADJ = ["Immersive", "Enhanced", "Better", "Lux", "Ultimate", "Simple", "Realistic"];
const NOUN = ["Armors", "Lighting", "Water", "Interface", "Combat", "Followers", "Textures", "Weather", "Animations", "Cities"];
const CATEGORY = ["Visuals and Graphics", "Gameplay", "User Interface", "Armour", "Immersion"];
const AUTHORS = ["Arthmoor", "Kojak747", "Sinitar", "Mur", "dTry", "Nolvus"];
const COLLECTIONS = ["Essentials", "Vanilla Plus", "Graphics Pack", "Lite Overhaul", "Survival Run", "Lore Friendly", "Performance First"];

interface Entry {
  name: string;
  author: string;
  category: string;
  downloads: number;
  endorsements: number;
  /** Days before now. */
  age: number;
}

/** Deterministic spread: the same catalogue on every run. */
const spread = (i: number, mod: number) => (i * 7919 + 104729) % mod;

const MODS: Entry[] = ADJ.flatMap((a, i) =>
  NOUN.map((n, j) => {
    const k = i * NOUN.length + j;
    return {
      name: `${a} ${n}`,
      author: AUTHORS[k % AUTHORS.length],
      category: CATEGORY[k % CATEGORY.length],
      downloads: 1000 + spread(k, 2_000_000),
      endorsements: 50 + spread(k + 3, 90_000),
      age: k % 9 === 0 ? spread(k, 6) : spread(k, 2400),
    };
  }),
);

const COLLS: Entry[] = COLLECTIONS.flatMap((c, i) =>
  ["", " Redux", " Next"].map((suffix, j) => {
    const k = i * 3 + j;
    return {
      name: `${c}${suffix}`,
      author: AUTHORS[(k + 2) % AUTHORS.length],
      category: CATEGORY[k % CATEGORY.length],
      downloads: 500 + spread(k, 400_000),
      endorsements: 10 + spread(k + 5, 9_000),
      age: k % 5 === 0 ? spread(k, 20) : spread(k, 1500),
    };
  }),
);

function select(all: Entry[], req: BrowseRequest): Entry[] {
  const days = PERIOD_DAYS[req.period];
  const words = req.text.toLowerCase().split(/\s+/).filter(Boolean);
  const key = (e: Entry) => (req.sort === "downloads" ? e.downloads : req.sort === "newest" ? -e.age : e.endorsements);
  return all
    .filter((e) => days === null || e.age <= days)
    .filter((e) => words.every((w) => e.name.toLowerCase().includes(w)))
    .sort((a, b) => key(b) - key(a));
}

/** Unique per entry (Nexus slugs are short and unique). */
const slugOf = (e: Entry) => `demo${COLLS.indexOf(e)}`;

/** One page of the demo catalogue; `installedMods` are Nexus mod ids, `installedCollections` slug -> revision. */
export function demoBrowse(
  req: BrowseRequest,
  installedMods: Set<number>,
  installedCollections: Map<string, number>,
): BrowseView {
  if (req.kind === "mods") {
    const hits = select(MODS, req);
    const mods: SearchHit[] = hits.slice(req.offset, req.offset + PAGE).map((e) => {
      const modId = 1000 + MODS.indexOf(e);
      return {
        modId,
        name: e.name,
        author: e.author,
        version: `${1 + (modId % 4)}.${modId % 10}`,
        summary: `${e.name} for the demo: a stand-in for what Nexus lists here.`,
        downloads: e.downloads,
        endorsements: e.endorsements,
        thumbnailUrl: `https://staticdelivery.nexusmods.com/demo/${modId}.png`,
        category: e.category,
        updatedAt: new Date(Date.now() - e.age * DAY).toISOString(),
        installed: installedMods.has(modId),
      };
    });
    return { kind: "mods", total: hits.length, mods };
  }
  const hits = select(COLLS, req);
  const collections: CollectionHit[] = hits.slice(req.offset, req.offset + PAGE).map((e) => {
    const slug = slugOf(e);
    const revision = 1 + (e.endorsements % 7);
    return {
      slug,
      name: e.name,
      summary: `A demo collection: ${e.name.toLowerCase()} in one install.`,
      author: e.author,
      category: e.category,
      endorsements: e.endorsements,
      downloads: e.downloads,
      revision,
      mod_count: 20 + (e.downloads % 300),
      total_size: (e.downloads % 40) * 1_000_000_000 + 500_000_000,
      image_url: `https://media.nexusmods.com/demo/${slug}.webp`,
      installed_revision: installedCollections.get(slug) ?? null,
    };
  });
  return { kind: "collections", total: hits.length, collections };
}
