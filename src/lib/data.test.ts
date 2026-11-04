import { renderHook, act, waitFor, cleanup } from "@testing-library/react";
import { it, expect, vi, beforeEach, afterEach } from "vitest";

type Cb = (e: { payload: { topic: string } }) => void;
let cb: Cb | null = null;
vi.mock("./ipc", () => ({
  isTauri: false,
  listen: vi.fn((_ev: string, f: Cb) => { cb = f; return Promise.resolve(() => { cb = null; }); }),
}));
import { useTopic } from "./data";
import { usePanelStore } from "../panel/store";

const fetcher = vi.fn();
const emit = (topic: string) => act(async () => { cb!({ payload: { topic } }); });
const setOpen = (open: boolean) => act(async () => { usePanelStore.setState({ open }); });

beforeEach(() => {
  cb = null;
  fetcher.mockReset();
  fetcher.mockResolvedValue("v");
  usePanelStore.setState({ open: false });
});
afterEach(cleanup);

it("ne charge pas tant que le panneau est fermé, charge à l'ouverture", async () => {
  const { result } = renderHook(() => useTopic("notes", fetcher, "init"));
  await waitFor(() => expect(cb).not.toBeNull());
  expect(fetcher).not.toHaveBeenCalled();
  expect(result.current.data).toBe("init");
  await setOpen(true);
  await waitFor(() => expect(result.current.data).toBe("v"));
  expect(fetcher).toHaveBeenCalledTimes(1);
});

it("recharge sur un événement du même sujet panneau ouvert, ignore les autres", async () => {
  usePanelStore.setState({ open: true });
  renderHook(() => useTopic("notes", fetcher, "init"));
  await waitFor(() => expect(fetcher).toHaveBeenCalledTimes(1));
  await emit("agenda");
  expect(fetcher).toHaveBeenCalledTimes(1);
  await emit("notes");
  await waitFor(() => expect(fetcher).toHaveBeenCalledTimes(2));
});

it("sujet périmé panneau fermé : recharge à la réouverture seulement", async () => {
  usePanelStore.setState({ open: true });
  renderHook(() => useTopic("notes", fetcher, "init"));
  await waitFor(() => expect(fetcher).toHaveBeenCalledTimes(1));
  await setOpen(false);
  await emit("notes");
  expect(fetcher).toHaveBeenCalledTimes(1);
  await setOpen(true);
  await waitFor(() => expect(fetcher).toHaveBeenCalledTimes(2));
  await setOpen(false);
  await setOpen(true);
  expect(fetcher).toHaveBeenCalledTimes(2);
});

it("deux rechargements qui se doublent : seul le dernier résultat compte", async () => {
  usePanelStore.setState({ open: true });
  let r1!: (v: string) => void;
  fetcher.mockReset();
  fetcher.mockImplementationOnce(() => new Promise((r) => { r1 = r; }));
  fetcher.mockResolvedValueOnce("second");
  const { result } = renderHook(() => useTopic("notes", fetcher, "init"));
  await waitFor(() => expect(fetcher).toHaveBeenCalledTimes(1));
  act(() => result.current.reload());
  await waitFor(() => expect(result.current.data).toBe("second"));
  await act(async () => { r1("first"); });
  expect(result.current.data).toBe("second");
});

it("clé changée panneau ouvert : recharge ; panneau fermé : périmé jusqu'à la réouverture", async () => {
  usePanelStore.setState({ open: true });
  const { rerender } = renderHook(({ k }) => useTopic("notes", fetcher, "init", k), { initialProps: { k: "2026-11-04" } });
  await waitFor(() => expect(fetcher).toHaveBeenCalledTimes(1));
  rerender({ k: "2026-11-04" });
  expect(fetcher).toHaveBeenCalledTimes(1);
  rerender({ k: "2026-11-05" });
  await waitFor(() => expect(fetcher).toHaveBeenCalledTimes(2));
  await setOpen(false);
  rerender({ k: "2026-11-06" });
  expect(fetcher).toHaveBeenCalledTimes(2);
  await setOpen(true);
  await waitFor(() => expect(fetcher).toHaveBeenCalledTimes(3));
});

it("échec du chargement : réessayé à la réouverture suivante", async () => {
  usePanelStore.setState({ open: true });
  fetcher.mockReset();
  fetcher.mockRejectedValueOnce(new Error("ipc")).mockResolvedValue("v");
  const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
  const { result } = renderHook(() => useTopic("notes", fetcher, "init"));
  await waitFor(() => expect(warn).toHaveBeenCalled());
  await setOpen(false);
  await setOpen(true);
  await waitFor(() => expect(result.current.data).toBe("v"));
  expect(fetcher).toHaveBeenCalledTimes(2);
  warn.mockRestore();
});
