import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { describe, expect, it } from "vitest";
import { type AssetLineComment } from "../../lib/sendAssetReview";
import { TextAssetLineReview } from "./TextAssetLineReview";

function Harness({ onChange }: { onChange: (c: AssetLineComment[]) => void }) {
  const [comments, setComments] = useState<AssetLineComment[]>([]);
  return (
    <TextAssetLineReview
      content={"one\ntwo\nthree\nfour"}
      comments={comments}
      onCommentsChange={(next) => {
        setComments(next);
        onChange(next);
      }}
    />
  );
}

describe("TextAssetLineReview", () => {
  it("drags across lines to comment on a range", async () => {
    const user = userEvent.setup();
    let latest: AssetLineComment[] = [];
    render(<Harness onChange={(next) => (latest = next)} />);
    await user.pointer([
      {
        keys: "[MouseLeft>]",
        target: screen.getByTestId("treq-send-text-line-2"),
      },
      { target: screen.getByTestId("treq-send-text-line-3") },
      { target: screen.getByTestId("treq-send-text-line-4") },
      { keys: "[/MouseLeft]" },
    ]);
    const composer = await screen.findByTestId(
      "treq-send-line-comment-composer",
    );
    await user.type(
      composer.querySelector("textarea") as HTMLTextAreaElement,
      "first{Shift>}{Enter}{/Shift}second",
    );
    await user.click(screen.getByTestId("treq-send-line-comment-save"));
    expect(latest).toHaveLength(1);
    expect(latest[0]).toMatchObject({
      startLine: 2,
      endLine: 4,
      lineContent: ["two", "three", "four"],
      text: "first\nsecond",
    });
  });

  it("extends the selection with Shift+click", async () => {
    const user = userEvent.setup();
    let latest: AssetLineComment[] = [];
    render(<Harness onChange={(next) => (latest = next)} />);
    await user.click(screen.getByTestId("treq-send-text-line-1"));
    await user.keyboard("{Shift>}");
    await user.click(screen.getByTestId("treq-send-text-line-3"));
    await user.keyboard("{/Shift}");
    await user.type(
      (
        await screen.findByTestId("treq-send-line-comment-composer")
      ).querySelector("textarea") as HTMLTextAreaElement,
      "range",
    );
    await user.click(screen.getByTestId("treq-send-line-comment-save"));
    expect(latest[0]).toMatchObject({ startLine: 1, endLine: 3 });
  });
});
