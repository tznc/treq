declare module "*.svg" {
  const markup: string;
  export default markup;
}

/** The extension APIs used here. */
declare const chrome: {
  storage: {
    sync: unknown;
    onChanged: { addListener(listener: (changes: object, area: string) => void): void };
  };
};
