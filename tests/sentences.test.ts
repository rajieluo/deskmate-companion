import { describe, expect, it } from "vitest";
import { SentenceBuffer, actionMoods, speakable } from "../src/sentences";

describe("SentenceBuffer", () => {
  it("emits sentences as they complete across chunks", () => {
    const b = new SentenceBuffer();
    expect(b.push("Hello there, how ")).toEqual([]);
    expect(b.push("are you today? I'm fine")).toEqual(["Hello there, how are you today?"]);
    expect(b.push(" thanks. ")).toEqual(["I'm fine thanks."]);
    expect(b.flush()).toEqual([]);
  });

  it("joins very short sentences onto the next one", () => {
    const b = new SentenceBuffer();
    expect(b.push("Oh! That is a great idea. ")).toEqual(["Oh! That is a great idea."]);
  });

  it("splits on newlines and flushes the remainder", () => {
    const b = new SentenceBuffer();
    expect(b.push("First line\nsecond part")).toEqual(["First line"]);
    expect(b.flush()).toEqual(["second part"]);
  });

  it("does not split decimals", () => {
    const b = new SentenceBuffer();
    expect(b.push("It costs 3.50 dollars today. ")).toEqual(["It costs 3.50 dollars today."]);
  });
});

describe("speakable", () => {
  it("drops markdown, emoji and URLs", () => {
    expect(speakable("**Sure!** See https://x.com/a 😀 `npm i`")).toBe("Sure! See a link npm i");
    expect(speakable("Read [Stripe's docs](https://docs.stripe.com/connect) first.")).toBe("Read Stripe's docs first.");
  });

  it("spells laughs so they are spoken as hee hee", () => {
    expect(speakable("hehe that's funny")).toBe("hee hee that's funny");
    expect(speakable("Hehe~ okay!")).toBe("Hee hee okay!");
    expect(speakable("HEHE, sure.")).toBe("Hee hee, sure.");
    expect(speakable("hehehe!")).toBe("hee hee hee!");
    expect(speakable("Well... hehe.")).toBe("Well... hee hee.");
  });

  it("drops *actions* so they are not read aloud", () => {
    expect(speakable("*stretches* I'm so sleepy.")).toBe("I'm so sleepy.");
    expect(speakable("Okay *shrug*, fine.")).toBe("Okay, fine.");
    expect(speakable("Sure thing *tilts head*!")).toBe("Sure thing!");
    expect(speakable("I did it *stretches* and now *nods* I'm done.")).toBe("I did it and now I'm done.");
  });

  it("still reads **bold** and long starred text", () => {
    expect(speakable("This is **really** important.")).toBe("This is really important.");
    expect(speakable("*this is a whole starred sentence for emphasis*")).toBe("this is a whole starred sentence for emphasis");
  });

  it("speaks a yawn as a drawn-out sound", () => {
    expect(speakable("*yawns*")).toBe("Haaaah...");
    expect(speakable("*YAWN* I'm sleepy.")).toBe("Haaaah... I'm sleepy.");
    expect(speakable("Okay *yawning*, bedtime.")).toBe("Okay Haaaah..., bedtime.");
  });

  it("speaks sighs, giggles and laughs as sound words", () => {
    expect(speakable("*sighs* Fine.")).toBe("Haah... Fine.");
    expect(speakable("*SIGHING*")).toBe("Haah...");
    expect(speakable("*giggles* You're silly.")).toBe("Hee hee You're silly.");
    expect(speakable("*giggling*")).toBe("Hee hee");
    expect(speakable("*Laughs* That's great!")).toBe("Ha hah! That's great!");
    expect(speakable("*laughing*")).toBe("Ha hah!");
  });

  it("speaks nothing for an action-only reply", () => {
    expect(speakable("*shrugs*")).toBe("");
    expect(speakable("*nods*.")).toBe("");
    expect(speakable(" *stretches* ... ")).toBe("");
  });

  it("finds moods for actions she can act out", () => {
    expect(actionMoods("*giggles* okay")).toEqual(["happy"]);
    expect(actionMoods("*sighs* fine *blushes*")).toEqual(["sad", "shy"]);
    expect(actionMoods("*yawns*")).toEqual([]);
    expect(actionMoods("**laugh** out loud")).toEqual([]);
  });

  it("leaves words that only contain hehe alone", () => {
    expect(speakable("thehe hehey heh haha")).toBe("thehe hehey heh haha");
  });
});
