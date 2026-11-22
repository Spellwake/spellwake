export type ReviewView = {
  id: string;
  title: string;
  image: string;
  width: number;
  height: number;
  notes: string[];
  actions: {
    label: string;
    outcome: string;
    target?: string;
    external?: string;
  }[];
};
export type ScreenProps = {
  section: string;
  view: ReviewView;
  data?: unknown;
  go: (id: string, data?: unknown) => void;
};
export type ReviewSection = { id: string; title: string; views: ReviewView[] };
