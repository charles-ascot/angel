export type WorkItemStatus = "active" | "blocked" | "completed";
export type EscalationLevel = "registry" | "notification" | "halt";

export interface WorkItem {
  id: string;
  title: string;
  description: string | null;
  status: WorkItemStatus;
  stream_tag: string | null;
  deep_link: string;
  first_seen_at: string;
  last_seen_at: string;
}

export interface PushbackEntry {
  id: string;
  triggered_at: string;
  new_session_path: string;
  matched_work_item_id: string | null;
  escalation_level: EscalationLevel;
  deep_link: string;
}
