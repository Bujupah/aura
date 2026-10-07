/**
 * Layout fixture only. Milestone 1 has no intelligence; this exists so the
 * expanded overlay can be designed against realistic content, and is shown
 * exclusively in dev builds, labelled as a sample.
 */
export interface OverlayRecommendation {
  readonly now: string;
  readonly askNext: string;
  readonly why: string;
}

export const sampleRecommendation: OverlayRecommendation = {
  now: "Customer is discussing CMDB accuracy.",
  askNext: "How do you currently reconcile CI information from different sources?",
  why: "This determines whether Discovery should become part of the architecture.",
};
