# T2: Nexus descriptions, what format they use, and how to render them

Research only. No code was changed. Checked on 2026-11-01 against the live `https://api.nexusmods.com/v2/graphql`. Schema introspection works without authentication. The samples come from the top 20 mods by endorsements in skyrimspecialedition, cyberpunk2077 and stardewvalley (60 mods), plus 4 collections.

## 1. GraphQL fields

| Entity | Query path | Arguments |
|---|---|---|
| Mod description (by domain) | `legacyModsByDomain(ids: [CompositeDomainWithIdInput!]!) { nodes { description } }` | `CompositeDomainWithIdInput = { gameDomain: String, modId: Int }` |
| Mod description (by numeric game id) | `mod(modId: ID!, gameId: ID!) { description }` | This is the form `mod_batch` already uses (`crates/vessel-core/src/nexus/graphql.rs:300-318`) |
| Mod short text | `Mod.summary` (already fetched) | |
| Collection description | `collection(slug: String, domainName: String, viewAdultContent: Boolean) { description }` | |
| Collection description, as part of the revision query | `collectionRevision(slug, domainName, revision, viewAdultContent) { collection { description } }` | Add `description` to `collection { ... }` in `REVISION_QUERY` (`crates/vessel-core/src/collections.rs:164`) |
| Revision changelog (optional) | `collectionRevision(...) { collectionChangelog { description } }` | Markdown |

The schema describes `Mod.description` as "A detailed description of this mod". The collection description belongs to the collection, not to a revision. Without `viewAdultContent: true`, an adult collection returns the error `extensions.code = "ADULT_CONTENT_BLOCKED"` (seen with slug `qdurkx`). Pass the `adult` setting through, as `BrowseQuery.adult` already does.

Neither browse query (`graphql.rs:18-29`) asks for `description`. The descriptions are 4 KB to 27 KB each, so fetch them on demand in the detail view, not in the list query.

### Sample payloads

Mod request:
```graphql
{ legacyModsByDomain(ids: [{gameDomain: "skyrimspecialedition", modId: 266}]) {
    nodes { modId name summary description } } }
```
Mod response:
```json
{"data":{"legacyModsByDomain":{"nodes":[{"modId":266,"name":"Unofficial Skyrim Special Edition Patch","summary":"...",
 "description":"[center][img]https://www.afkmods.com/modbanners/ussep-logo.jpg[/img][/center]\n<br />[center][color=red][size=6]Requires Skyrim Special Edition 1.7.99 or greater.[/size][/color][/center]\n<br />..."}]}}}
```
Collection request:
```graphql
{ collectionRevision(slug: "qdurkx", domainName: "skyrimspecialedition", viewAdultContent: true) {
    revisionNumber collectionChangelog { description } collection { slug name summary description } } }
```
Collection response:
```json
{"data":{"collectionRevision":{"revisionNumber":118,
 "collectionChangelog":{"description":"V118 is finally here...\n\nYou can read the [full list of changes here](https://docs.google.com/...).\n\n**Don't forget to endorse...**\n\nHow to update to V118:&#x20;\n\n- Make sure you ..."},
 "collection":{"slug":"qdurkx","name":"Gate To Sovngarde","summary":"...","description":"..."}}}}
```

## 2. Markup formats

### Mods use the Nexus BBCode dialect plus HTML line breaks

In all 60 sampled mods, every newline is written as `\n<br />`. Line breaks therefore have two parts:
- Treat `<br />` as the line break.
- Drop the `\n` that comes before it.

`<br />` is the only HTML tag that appeared.

| Tag | Mods using it (of 60) | Examples |
|---|---|---|
| `[url=...]...[/url]` | 57 | `[url=https://smapi.io]Install the latest version of SMAPI[/url]`. The bare `[url]x[/url]` form did not appear but should still be supported. |
| `[b]` `[i]` `[u]` `[s]` | 55 / 26 / 17 / 4 | |
| `[size=N]` | 52 | N is 1 to 6 |
| `[color=...]` | 42 | Named (`[color=Red]`) or hex (`[color=#6aa84f]`) |
| `[list]` / `[list=1]` with `[*]` | 37 | Items have no closing tag |
| `[img]...[/img]` | 31 | Also `[img width=20,height=20]https://smapi.io/...svg[/img]`. Hosts include imgur, github raw, afkmods and staticdelivery.nexusmods.com. |
| `[center]` `[left]` | 19 / 2 | `[right]` should be expected as well |
| `[font=...]` | 13 | `Courier New`, `Comic Sans MS`, `bebas_neuebook` |
| `[youtube]ID[/youtube]` | 11 | `[youtube]9DXBti-c6C0[/youtube]`. The content is a bare video ID. |
| `[spoiler]...[/spoiler]` | 10 | Only the form without an argument appeared |
| `[code]` | 7 | `[code]"OpenMenuKey": "P, LeftShoulder"...` |
| `[line]` | 4 | A horizontal rule, with no closing tag |
| `[quote]` | 2 | Only the form without an argument appeared |

The text also contains bracketed words that are not tags, such as `[any]`, `[the]`, `[cp]`, `[memory]` and `[FTM]`. Unknown tags must be printed as literal text.

HTML entities appear inside the text: `&#92;` (backslash, in 19 mods, for example `C:&#92;Program Files (x86)&#92;Steam`), `&gt;`, `&amp;` and `&lt;`. Decode the named entities and numeric `&#N;` / `&#xH;` entities into text. Never parse them as markup.

### Collections use Markdown (CommonMark plus GFM)

This applies to both `collection.description` and `collectionChangelog.description`. The text contains:
- headings (`###`)
- emphasis (`**...**`)
- links (`[text](url)`)
- images (`![](https://i.imgur.com/...png)`), including images inside links (`[![](img)](https://ko-fi.com/...)`)
- lists (`- `)
- blockquotes (`> `)
- blank-line paragraphs

Raw HTML also appears: `<details><summary>...</summary>...</details>` in 3 of 3 sampled cyberpunk collections (4 to 19 times each), and once a `<button>`. The entity `&#x20;` appears as a trailing space. There is no BBCode.

## 3. Rendering recommendation

**Approach:** use renderers that build React elements. Neither one uses `dangerouslySetInnerHTML`.

- **Mods (BBCode):** write an in-repo renderer, for example `apps/desktop/src/lib/bbcode.tsx`.
  - Tokenize the text into a tag stack, using an allowlist of the tags in the table above.
  - Print unknown or unbalanced tags as literal text.
  - Decode entities into text nodes.
  - Turn `\n?<br\s*/?>` into `<br/>`.
  - The allowlist needs about 15 tags, so this should be roughly 200 to 300 lines plus vitest cases built from the samples above.
- **Collections (Markdown):** use `react-markdown@10.1.0` with `remark-gfm@4.0.1`, pinned exactly the way `package.json` pins its other dependencies.
  - Do not add `rehype-raw`. By default, react-markdown drops raw HTML. For `<details>`, the summary and body text then show as plain text, which is acceptable.
  - Override the `a` and `img` components as described under CSP compatibility below.

Why not `@bbob/react@4.4.1` with `@bbob/preset-react`:
- Its dependencies `@bbob/core`, `@bbob/html`, `@bbob/plugin-helper` and `@bbob/types` are declared as `*`, so they cannot be pinned.
- Its default preset does not know the Nexus tags `[spoiler]`, `[youtube]`, `[line]`, `[size=1-6]` or `[font]`. A custom preset would be needed anyway, which is about as much work as the in-repo renderer.
- It has no `\n<br />` handling.

### CSP compatibility (`tauri.conf.json:26`: `img-src 'self' data:; style-src 'self'`)

- **Images.** Remote image URLs are blocked. Render `[img]` and `![]()` with a component that calls `ipc.picture(url)` (`commands.ts:254`), which returns a `data:` URL.
  - `picture` only accepts https URLs on `nexusmods.com` and `nexus-cdn.com` hosts (`crates/vessel-core/src/app/thumbs.rs:45-54`) and returns `null` for any other URL.
  - Imgur, github and afkmods images (most of the samples) will therefore fail. For those, show a link placeholder such as "image: host", or widen the allowlist with a size cap as a separate decision.
- **YouTube.** Render `[youtube]` as a link to `https://www.youtube.com/watch?v=ID`. Iframes are blocked because no `frame-src` is set, so `default-src 'self'` applies.
- **`[size]`, `[color]` and `[font]`.**
  - React applies `style={{}}` through the CSSOM, which CSP does not block.
  - Mapping the values to Tailwind classes is still safer and keeps the theme readable: `size1` to `size6` become classes, and `color` is either ignored or limited to a small palette, since red text on a dark theme is often unreadable.
  - Ignore `[font]`.
- **Spoilers.** Render `[spoiler]` as `<details><summary>Spoiler</summary>...</details>`.
- **No inline styles or event-handler attributes in the output.** Because only React elements are emitted, no raw strings reach the DOM.

### Opening external links

- **What exists today:** `mod_open_page` / `allow-mod-open-page`.
  - Rust: `apps/desktop/src-tauri/src/commands.rs:354-367` calls `appmods::open_mod_page` (`crates/vessel-core/src/app/mods.rs:252-265`).
  - It only opens the Nexus page of an **installed** mod, resolved by reference from the database. The URL comes from `nexus_page_url`, and the page is opened with `appgames::launch_opener` (xdg-open or gio, `crates/vessel-core/src/app/games.rs:425`).
  - It cannot open an arbitrary URL.
- **Frontend pattern:** `apps/desktop/src/views/Mods.tsx:511-525` renders `<a href>` and calls `e.preventDefault()` so the webview does not navigate, then calls `ipc.modOpenPage`.
- **Gap:** the app has no opener or shell plugin and no general URL-opening command. Links inside descriptions, and the pages of browsed mods that are not installed, need a new command such as `open_url(url)` (with an `allow-open-url` permission) that reuses `launch_opener`.
  - It should accept only `https:`, and possibly `http:`, URLs.
  - It must reject `file:`, `javascript:` and similar schemes before calling xdg-open.
- **Link component:** every link in both renderers must use the `preventDefault` pattern and route through that command.
