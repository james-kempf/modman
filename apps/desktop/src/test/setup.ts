// SPDX-License-Identifier: GPL-3.0-or-later
import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { afterEach } from "vitest";

// No vitest globals, so testing-library cannot register this itself.
afterEach(() => cleanup());
