import { render, screen, cleanup } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { it, expect, vi, afterEach, beforeEach } from "vitest";
import { Briefing } from "./Briefing";
import { irisBriefingIpc, type BriefingData } from "./api";
import { usePanelStore } from "../../panel/store";

afterEach(() => { cleanup(); vi.restoreAllMocks(); });
beforeEach(() => usePanelStore.setState({ open: true }));

const data: BriefingData = {
  segments: [{ t: "Text", text: "Prochain : " }, { t: "Untrusted", text: "Réunion piégée", source: "agenda externe" }, { t: "Text", text: " à 14 h." }],
  suggestions: [{ kind: "start_focus", label: "Lancer un focus" }, { kind: "open_todo", label: "Voir les mails à traiter" }],
  at_ms: 1,
};

it("le texte d'un tiers est stylé (italique, source en infobulle) et le texte d'IRIS ne l'est pas", async () => {
  vi.spyOn(irisBriefingIpc, "get").mockResolvedValue(data);
  const { container } = render(<Briefing />);
  const q = await screen.findByText("Réunion piégée");
  expect(q).toHaveClass("iris-untrusted");
  expect(q).toHaveAttribute("title", "Événement externe, non fiable");
  expect(container.querySelectorAll(".iris-untrusted")).toHaveLength(1);
});

it("le clic sur une suggestion appelle iris_briefing_act avec son rang", async () => {
  vi.spyOn(irisBriefingIpc, "get").mockResolvedValue(data);
  const act = vi.spyOn(irisBriefingIpc, "act").mockResolvedValue();
  render(<Briefing />);
  await userEvent.click(await screen.findByRole("button", { name: "Voir les mails à traiter" }));
  expect(act).toHaveBeenCalledWith(1, data.at_ms);
});

it("briefing remplacé entre-temps : message dédié", async () => {
  vi.spyOn(irisBriefingIpc, "get").mockResolvedValue(data);
  vi.spyOn(irisBriefingIpc, "act").mockRejectedValue("briefing_stale");
  render(<Briefing />);
  await userEvent.click(await screen.findByRole("button", { name: "Lancer un focus" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Briefing mis à jour, réessaie.");
});

it("action en échec : message, sans planter", async () => {
  vi.spyOn(irisBriefingIpc, "get").mockResolvedValue(data);
  vi.spyOn(irisBriefingIpc, "act").mockRejectedValue("action_failed");
  render(<Briefing />);
  await userEvent.click(await screen.findByRole("button", { name: "Lancer un focus" }));
  expect(await screen.findByRole("alert")).toHaveTextContent("Action impossible");
});
