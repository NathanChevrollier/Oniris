import { render } from "@testing-library/react";
import { it, expect } from "vitest";
import { Orb } from "./Orb";

it("Orb expose son état et son niveau", () => {
  const { container } = render(<Orb state="listening" size={58} level={0.4} />);
  const el = container.firstElementChild as HTMLElement;
  expect(el.dataset.state).toBe("listening");
  expect(el.style.getPropertyValue("--level")).toBe("0.4");
});
