import type { WorkItem } from "../types";

interface Props {
  item: WorkItem;
}

function relativeTime(iso: string): string {
  const diff = Date.now() - new Date(iso).getTime();
  const m = Math.floor(diff / 60_000);
  const h = Math.floor(m / 60);
  const d = Math.floor(h / 24);
  if (d > 0) return `${d}d ago`;
  if (h > 0) return `${h}h ago`;
  if (m > 0) return `${m}m ago`;
  return "just now";
}

export function WorkItemCard({ item }: Props) {
  return (
    <div className="card">
      <div className="card-title">{item.title}</div>

      {item.stream_tag && (
        <span className="card-tag">{item.stream_tag}</span>
      )}

      {item.description && (
        <div className="card-description">{item.description}</div>
      )}

      <div className="card-footer">
        <span className="card-time">{relativeTime(item.last_seen_at)}</span>
        <a
          className="card-link"
          href={item.deep_link}
          title={item.deep_link}
          onClick={(e) => {
            // angel:// URIs are not browser-navigable yet — prevent default
            // until the URL scheme handler is registered.
            if (item.deep_link.startsWith("angel://")) {
              e.preventDefault();
              navigator.clipboard
                .writeText(item.deep_link)
                .catch(() => undefined);
            }
          }}
        >
          {item.deep_link}
        </a>
      </div>
    </div>
  );
}
