import { describe, expect, it, vi } from "vitest";

vi.mock("../src/host", () => ({ openUrl: vi.fn() }));
const { findLinks } = await import("../src/links");

const urls = (text: string) => findLinks(text).filter((p) => p.url).map((p) => p.url);

describe("findLinks", () => {
  it("finds full addresses and drops trailing punctuation", () => {
    expect(urls("See https://docs.stripe.com/connect.")).toEqual(["https://docs.stripe.com/connect"]);
    expect(urls("(at https://x.com/a)")).toEqual(["https://x.com/a"]);
    expect(urls("https://en.wikipedia.org/wiki/Foo_(bar)")).toEqual(["https://en.wikipedia.org/wiki/Foo_(bar)"]);
  });

  it("finds bare addresses", () => {
    expect(urls("go to dashboard.stripe.com/settings/connect, then")).toEqual(["https://dashboard.stripe.com/settings/connect"]);
  });

  it("uses the label of markdown links", () => {
    expect(findLinks("Read [the docs](https://docs.stripe.com) now")).toEqual([
      { text: "Read " },
      { text: "the docs", url: "https://docs.stripe.com" },
      { text: " now" },
    ]);
  });

  it("leaves email addresses and file names alone", () => {
    expect(urls("mail me@example.com about main.ts")).toEqual([]);
  });

  it("keeps all the text", () => {
    const text = "One https://a.com/x, two b.org. Done!";
    expect(findLinks(text).map((p) => p.text).join("")).toBe(text);
  });
});
