import { render, screen, cleanup } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { it, expect, vi, afterEach } from "vitest";
import { RoutinesSection } from "./RoutinesSection";
import { routinesIpc, type Routine } from "./api";
import { defaultArgs, missingField } from "./tools";

afterEach(() => { cleanup(); vi.restoreAllMocks(); });

const jeu: Routine = { name: "Mode jeu", phrases: ["on joue"], steps: [{ tool: "pc.volume", args: { percent: 40 }, blocking: false }, { tool: "apps.launch", args: { key: "Steam" }, blocking: true }] };

it("liste, lance et supprime après confirmation", async () => {
  vi.spyOn(routinesIpc, "list").mockResolvedValue([jeu]);
  const run = vi.spyOn(routinesIpc, "run").mockResolvedValue({ summary: "Routine « Mode jeu » : 2 étapes sur 2 faites." });
  const save = vi.spyOn(routinesIpc, "save").mockResolvedValue([]);
  render(<RoutinesSection />);
  expect(await screen.findByText("Mode jeu")).toBeTruthy();
  await userEvent.click(screen.getByRole("button", { name: "Lancer" }));
  expect(run).toHaveBeenCalledWith("Mode jeu");
  expect(await screen.findByText(/2 étapes sur 2 faites/)).toBeTruthy();
  await userEvent.click(screen.getByRole("button", { name: "Supprimer Mode jeu" }));
  expect(save).not.toHaveBeenCalled();
  await userEvent.click(screen.getByRole("button", { name: "Confirmer la suppression" }));
  expect(save).toHaveBeenCalledWith([]);
});

it("création : champ obligatoire vérifié avant l'enregistrement", async () => {
  vi.spyOn(routinesIpc, "list").mockResolvedValue([]);
  const save = vi.spyOn(routinesIpc, "save").mockImplementation(async (l) => l);
  render(<RoutinesSection />);
  await userEvent.click(await screen.findByRole("button", { name: "Nouvelle routine" }));
  await userEvent.type(screen.getByPlaceholderText("Mode jeu"), "Mode boulot");
  await userEvent.click(screen.getByRole("button", { name: "Enregistrer" }));
  expect(await screen.findByRole("alert")).toHaveProperty("textContent", "Étape 1 : « Application » est obligatoire.");
  await userEvent.type(screen.getByPlaceholderText("Steam"), "Code");
  await userEvent.click(screen.getByRole("button", { name: "Enregistrer" }));
  expect(save).toHaveBeenCalledWith([{ name: "Mode boulot", phrases: [], steps: [{ tool: "apps.launch", args: { key: "Code" }, blocking: false }] }]);
});

it("arguments par défaut", () => {
  expect(defaultArgs("projects.resume")).toEqual({ mode: "normal" });
  expect(defaultArgs("pc.bluetooth")).toEqual({ on: true });
  expect(defaultArgs("apps.move")).toEqual({ screen: "gauche" });
  expect(defaultArgs("apps.launch")).toEqual({});
  expect(missingField("music.toggle", {})).toBeNull();
});
