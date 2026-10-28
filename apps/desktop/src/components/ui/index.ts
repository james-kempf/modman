// SPDX-License-Identifier: GPL-3.0-or-later
// UI primitives in the legacy web UI's visual language (soft surfaces, chips,
// Phosphor icons). Large (>= 44px) targets, no hover-only affordances, no
// inline styles (CSP style-src 'self').
// Barrel: each primitive lives in its own file; import from here.

export { Badge } from "./display/Badge";
export { Button } from "./controls/Button";
export { Card } from "./layout/Card";
export { CardSection } from "./layout/CardSection";
export { Empty } from "./feedback/Empty";
export { ErrorNote } from "./feedback/ErrorNote";
export { Fact } from "./display/Fact";
export { IconButton } from "./controls/IconButton";
export { ListToggle } from "./controls/ListToggle";
export { MsgLine } from "./display/MsgLine";
export { Row } from "./layout/Row";
export { Segmented } from "./controls/Segmented";
export { Select } from "./controls/Select";
export { StatusDot, type DotTone } from "./display/StatusDot";
export { SupportBadge } from "./display/SupportBadge";
export { TextField } from "./controls/TextField";
export { Thumb } from "./display/Thumb";
export { Toggle } from "./controls/Toggle";
export { ViewHeader } from "./layout/ViewHeader";
export { ago, bytes, monogram, when } from "./format";
export {
  CARD_SHELL,
  CARD_TITLE,
  DIVIDE,
  INSET_HEADER_PAD,
  LIST_ROW,
  MUTED_NOTE,
  PILL,
  SECTION_PAD,
  TILE,
  cx,
} from "./styles";
