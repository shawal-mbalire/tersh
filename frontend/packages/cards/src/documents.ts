// Where HTML card documents are served from. Each runs in a sandboxed iframe;
// in a web page a `blob:` URL is enough, while tersh serves them from its own
// `tersh-card:` origin. Kept separate from the lazily loaded embed code.

export interface DocumentHost {
  put(id: string, html: string): Promise<void>;
  drop(id: string): void;
  /** URL an iframe loads the document from. */
  url(id: string): string;
}

/** Serves documents as `blob:` URLs. */
export function blobDocumentHost(): DocumentHost {
  const urls = new Map<string, string>();
  return {
    async put(id, html) {
      urls.set(id, URL.createObjectURL(new Blob([html], { type: "text/html" })));
    },
    drop(id) {
      const url = urls.get(id);
      if (url) URL.revokeObjectURL(url);
      urls.delete(id);
    },
    url: (id) => urls.get(id) ?? "about:blank",
  };
}

let current: DocumentHost | null = null;

/** Replace where HTML documents are served from (tersh uses its own protocol). */
export function setDocumentHost(host: DocumentHost) {
  current = host;
}

export function documentHost(): DocumentHost {
  return (current ??= blobDocumentHost());
}
