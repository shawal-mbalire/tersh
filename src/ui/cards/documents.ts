// Where HTML card documents are stored. Set once by the composition root;
// kept separate from the lazily loaded embed code.

import type { CardDocuments } from "../../ports/backend";

let current: CardDocuments | null = null;

export function setCardDocuments(documents: CardDocuments) {
  current = documents;
}

export function cardDocuments(): CardDocuments {
  if (!current) throw new Error("no card document store configured");
  return current;
}
