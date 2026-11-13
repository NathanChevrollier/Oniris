import { render } from "@testing-library/react";
import { it, expect } from "vitest";
import { Orb } from "./Orb";

it("Orb expose son état et son niveau", () => {
  const { container } = render(<Orb state="listening" size={58} level={0.4} />);
  const el = container.firstElementChild as HTMLElement;
  expect(el.dataset.state).toBe("listening");
  expect(el.style.getPropertyValue("--level")).toBe("0.4");
});

it("Orb pose ses calques (halo, ondes, trois blobs) et reste décorative", () => {
  const { container } = render(<Orb state="idle" size={24} />);
  const el = container.firstElementChild as HTMLElement;
  expect(el).toHaveAttribute("aria-hidden", "true");
  expect(el.querySelector(".glow")).not.toBeNull();
  expect(el.querySelectorAll(".orb-waves i")).toHaveLength(2);
  expect(el.querySelectorAll(".orb-swirl i")).toHaveLength(3);
});
