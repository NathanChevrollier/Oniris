import { invoke } from "../../lib/ipc";

/** Miroir de `modules::onboarding::Onboarding` : `step` = écran courant (1 à 5, 0 = pas commencé). */
export type Onboarding = { step: number; done: boolean; autostart_consent: boolean | null };
export type OnboardingView = Onboarding & { suggested_root: string | null };

export const onboardingIpc = {
  get: () => invoke<OnboardingView>("onboarding_get"),
  set: (onboarding: Onboarding) => invoke<void>("onboarding_set", { onboarding }),
};
