/**
 * The deck builder review reads its card printings from the review artwork in
 * the shared workspace (`output/imagegen/…`). A checkout of the code alone has
 * none of it, so the two catalogues' shapes are declared here and the gallery
 * still type checks. Where the files exist, they are read as they are.
 */
declare module "*/components/assets/catalog.json" {
  const printings: {
    name: string;
    type: string;
    mana: string;
    mana_value: number;
    rules: string;
    artist: string;
    image: string;
    art_image: string;
    source: string;
    image_source: string;
    color_identity: string[];
    legalities: Record<string, string>;
  }[];
  export default printings;
}

declare module "*/components/assets/search-cards/catalog.json" {
  const printings: {
    name: string;
    type: string;
    mana: string;
    mana_value: number;
    artist: string;
    image: string;
    art_image: string;
    source: string;
    image_source: string;
    art_source: string;
    colors: string[];
  }[];
  export default printings;
}
