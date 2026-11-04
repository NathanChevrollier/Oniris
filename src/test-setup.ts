import "@testing-library/jest-dom/vitest";

// jsdom n'a pas de ResizeObserver (CardGrid en utilise un) : version inerte.
globalThis.ResizeObserver ??= class { observe() {} unobserve() {} disconnect() {} } as unknown as typeof ResizeObserver;
