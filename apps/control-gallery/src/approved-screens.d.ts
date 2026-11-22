declare module "virtual:approved-screens" {
  const sections: import("./review-types").ReviewSection[];
  export default sections;
}

declare module "virtual:deck-import-catalogue" {
  const rows: string;
  export default rows;
}
