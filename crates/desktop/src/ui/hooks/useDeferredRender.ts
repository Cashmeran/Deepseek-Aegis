// Deferred render — skip rendering collapsed sections to keep the
// viewport responsive. Uses CSS content-visibility for native laziness.
// Adapted from deepseek-gui's use-deferred-render.

import { useRef, useCallback } from "react";

interface DeferredRenderOptions {
  /** Whether the content is expanded/visible. */
  enabled: boolean;
  /** Skip deferral — render immediately regardless of enabled. */
  immediate?: boolean;
  /** Optional viewport element ref for scroll-based deferral. */
  root?: React.RefObject<HTMLElement | null>;
}

interface DeferredRenderResult {
  ref: React.RefCallback<HTMLDivElement>;
  shouldRender: boolean;
}

export function useDeferredRender<T extends HTMLElement>(opts: DeferredRenderOptions): DeferredRenderResult {
  const { enabled, immediate = false } = opts;
  const shouldRender = immediate || enabled;

  const ref = useCallback(
    (node: T | null) => {
      if (!node) return;
      if (!enabled && !immediate) {
        // Use CSS containment — browser skips layout/paint when collapsed
        node.style.contentVisibility = "auto";
        node.style.containIntrinsicSize = "auto 220px";
      } else {
        node.style.contentVisibility = "";
        node.style.containIntrinsicSize = "";
      }
    },
    [enabled, immediate]
  );

  return { ref, shouldRender };
}
