// SPDX-License-Identifier: GPL-3.0-or-later
// Exhaustiveness guard: a switch default that fails typecheck when a union
// grows a case nobody handles.

export function assertNever(x: never): never {
  throw new Error(`unhandled case: ${JSON.stringify(x)}`);
}
