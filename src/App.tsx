import { useEffect, useRef, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import "./App.css";

type WorktreeInfo = {
  name: string;
  path: string;
  branch: string | null;
  head_oid: string | null;
  is_main: boolean;
  ahead: number | null;
  behind: number | null;
};

type Segment = { text: string; emphasized: boolean };

type DiffLineTag = "equal" | "delete" | "insert" | "gap";

type DiffLine = {
  tag: DiffLineTag;
  old_lineno: number | null;
  new_lineno: number | null;
  segments: Segment[];
  skipped: number | null;
};

type FileDiff = {
  path: string;
  status: string;
  additions: number;
  deletions: number;
  section: "committed" | "uncommitted";
  binary: boolean;
  lines: DiffLine[];
};

type DiffResult = {
  merge_base_oid: string;
  head_oid: string;
  files: FileDiff[];
};

type Row = { gap: true } | { gap: false; left: DiffLine | null; right: DiffLine | null };

type CommitInfo = {
  oid: string;
  short_oid: string;
  summary: string;
  body: string;
  author_name: string;
  author_email: string;
  timestamp: number;
  parent_oids: string[];
};

type GraphRow = {
  commit: CommitInfo;
  lane: number;
  /** Lanes with a straight line passing through this row untouched (not this commit's own lane). */
  passThrough: number[];
  /** Other lanes converging into this commit's lane at this row (extra parents already tracked,
   * or ancestor collisions) — drawn as a curve joining up into `lane`. */
  convergeFrom: number[];
  /** New lanes spawned by this commit's extra parents (merge branches not seen yet) — drawn as a
   * curve leaving `lane` heading to each new lane below. */
  divergeTo: number[];
  /** Whether this commit's own lane continues downward (has a first parent still to walk). */
  continues: boolean;
  maxLane: number;
};

/** Assigns each commit a vertical "lane" so the graph can be drawn as git log --graph does:
 * a lane holds one branch of history: a straight line unless a merge commit (multiple parents)
 * spawns a new lane for the branch it merged in, which later re-converges when that lane's
 * history reaches a commit another lane is also waiting for. */
function computeGraphRows(commits: CommitInfo[]): GraphRow[] {
  // lanes[k] = oid this lane is waiting to reach next, or null if the lane is free.
  const lanes: (string | null)[] = [];
  const rows: GraphRow[] = [];

  for (const commit of commits) {
    const matches: number[] = [];
    lanes.forEach((waiting, i) => {
      if (waiting === commit.oid) matches.push(i);
    });

    let lane: number;
    if (matches.length > 0) {
      lane = matches[0];
      for (const extra of matches.slice(1)) lanes[extra] = null;
    } else {
      lane = lanes.findIndex((x) => x === null);
      if (lane === -1) lane = lanes.length;
    }

    const passThrough: number[] = [];
    lanes.forEach((waiting, i) => {
      if (i !== lane && waiting !== null && !matches.includes(i)) passThrough.push(i);
    });

    const [firstParent, ...extraParents] = commit.parent_oids;
    lanes[lane] = firstParent ?? null;

    const divergeTo: number[] = [];
    for (const parent of extraParents) {
      const existing = lanes.findIndex((x) => x === parent);
      if (existing !== -1) continue; // already tracked; will converge naturally later
      const free = lanes.findIndex((x) => x === null);
      const newLane = free === -1 ? lanes.length : free;
      lanes[newLane] = parent;
      divergeTo.push(newLane);
    }

    rows.push({
      commit,
      lane,
      passThrough,
      convergeFrom: matches.slice(1),
      divergeTo,
      continues: firstParent !== undefined,
      maxLane: Math.max(lane, ...passThrough, ...divergeTo, 0),
    });
  }

  return rows;
}

/** Pairs delete/insert runs side by side so they render as aligned old|new columns. */
function buildRows(lines: DiffLine[]): Row[] {
  const rows: Row[] = [];
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    if (line.tag === "gap") {
      rows.push({ gap: true });
      i++;
      continue;
    }
    if (line.tag === "equal") {
      rows.push({ gap: false, left: line, right: line });
      i++;
      continue;
    }
    const deletes: DiffLine[] = [];
    while (i < lines.length && lines[i].tag === "delete") deletes.push(lines[i++]);
    const inserts: DiffLine[] = [];
    while (i < lines.length && lines[i].tag === "insert") inserts.push(lines[i++]);
    const max = Math.max(deletes.length, inserts.length);
    for (let k = 0; k < max; k++) {
      rows.push({ gap: false, left: deletes[k] ?? null, right: inserts[k] ?? null });
    }
  }
  return rows;
}

function Cell({ line, side }: { line: DiffLine | null; side: "left" | "right" }) {
  const lineno = side === "left" ? line?.old_lineno : line?.new_lineno;
  const linenoClass = line ? `lineno lineno--${line.tag}` : "lineno";
  const tagClass = line ? `diff-cell diff-cell--${line.tag}` : "diff-cell";
  return (
    <>
      <span className={linenoClass}>{lineno ?? ""}</span>
      <span className={tagClass}>
        {line?.segments.map((s, i) => (
          <span key={i} className={s.emphasized ? "diff-emphasis" : undefined}>
            {s.text}
          </span>
        ))}
      </span>
    </>
  );
}

function SideBySideDiff({ file }: { file: FileDiff }) {
  if (file.binary) return <p className="diff-note">Binary file, no preview.</p>;
  const rows = buildRows(file.lines);
  return (
    <div className="diff-grid">
      {rows.map((row, i) =>
        row.gap ? (
          <div className="diff-gap" key={i}>
            ⋯ unchanged ⋯
          </div>
        ) : (
          <div className="diff-row" key={i}>
            <Cell line={row.left} side="left" />
            <Cell line={row.right} side="right" />
          </div>
        ),
      )}
    </div>
  );
}

/** Drag handle between two panels; reports the delta in px while dragging. */
function Resizer({ onResize }: { onResize: (deltaX: number) => void }) {
  const dragging = useRef(false);
  const lastX = useRef(0);

  function onMouseDown(e: React.MouseEvent) {
    dragging.current = true;
    lastX.current = e.clientX;
    const onMouseMove = (ev: MouseEvent) => {
      if (!dragging.current) return;
      onResize(ev.clientX - lastX.current);
      lastX.current = ev.clientX;
    };
    const onMouseUp = () => {
      dragging.current = false;
      window.removeEventListener("mousemove", onMouseMove);
      window.removeEventListener("mouseup", onMouseUp);
    };
    window.addEventListener("mousemove", onMouseMove);
    window.addEventListener("mouseup", onMouseUp);
  }

  return <div className="resizer" onMouseDown={onMouseDown} />;
}

/** Drag handle for the bottom panel's height. */
function VerticalResizer({ onResize }: { onResize: (deltaY: number) => void }) {
  const dragging = useRef(false);
  const lastY = useRef(0);

  function onMouseDown(e: React.MouseEvent) {
    dragging.current = true;
    lastY.current = e.clientY;
    const onMouseMove = (ev: MouseEvent) => {
      if (!dragging.current) return;
      onResize(ev.clientY - lastY.current);
      lastY.current = ev.clientY;
    };
    const onMouseUp = () => {
      dragging.current = false;
      window.removeEventListener("mousemove", onMouseMove);
      window.removeEventListener("mouseup", onMouseUp);
    };
    window.addEventListener("mousemove", onMouseMove);
    window.addEventListener("mouseup", onMouseUp);
  }

  return <div className="vertical-resizer" onMouseDown={onMouseDown} />;
}

const GRAPH_PAGE_SIZE = 100;
const LANE_WIDTH = 16;
const LANE_X0 = 10;
// ponytail: fixed rather than derived from the loaded commits' real max lane count — sizing the
// column to the data made the message/author/date columns visibly shift every time pagination
// loaded a page with more (or fewer) concurrent branches. A rare graph busier than this still
// draws correctly (SVG isn't clipped), it just extends past its own column into the message text.
const GRAPH_MAX_LANES = 8;
const GRAPH_LANE_COLUMN_WIDTH = GRAPH_MAX_LANES * LANE_WIDTH + LANE_X0;
const LANE_COLORS = ["#a08256", "#7fa87f", "#a87f7f", "#8a8fbf", "#bf8fbf", "#8fb0bf"];

function laneColor(lane: number) {
  return LANE_COLORS[lane % LANE_COLORS.length];
}

function formatRelativeTime(timestampSeconds: number): string {
  const diffMs = Date.now() - timestampSeconds * 1000;
  const minutes = Math.round(diffMs / 60000);
  if (minutes < 1) return "just now";
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.round(hours / 24);
  if (days < 30) return `${days}d ago`;
  const months = Math.round(days / 30);
  if (months < 12) return `${months}mo ago`;
  return `${Math.round(months / 12)}y ago`;
}

/** A thin visible stroke has a hit area too small to hover reliably, and at busy convergence
 * points the old "hover the whole row" approach highlighted the row's own lane no matter which
 * line the cursor was actually over. Each segment now gets its own wide, invisible, transparent
 * twin purely for hit-testing (`pointerEvents="stroke"` so only the drawn path counts, not its
 * bounding box), so hovering highlights exactly the line under the cursor. */
function GraphLane({
  row,
  hoveredLane,
  onHoverLane,
}: {
  row: GraphRow;
  hoveredLane: number | null;
  onHoverLane: (lane: number | null) => void;
}) {
  const x = (lane: number) => LANE_X0 + lane * LANE_WIDTH;
  const opacity = (...lanes: number[]) => (hoveredLane === null || lanes.includes(hoveredLane) ? 1 : 0.22);
  const hitProps = (lane: number) => ({
    stroke: "transparent",
    strokeWidth: 10,
    pointerEvents: "stroke" as const,
    onMouseEnter: () => onHoverLane(lane),
    onMouseLeave: () => onHoverLane(null),
  });
  return (
    <svg width={GRAPH_LANE_COLUMN_WIDTH} height={36} className="graph-lane" style={{ overflow: "visible" }}>
      {row.passThrough.map((lane) => (
        <g key={`p${lane}`}>
          <line x1={x(lane)} y1={0} x2={x(lane)} y2={36} stroke={laneColor(lane)} strokeWidth={2} opacity={opacity(lane)} pointerEvents="none" />
          <line x1={x(lane)} y1={0} x2={x(lane)} y2={36} {...hitProps(lane)} />
        </g>
      ))}
      {/* incoming line from above, unless this lane just started at this row */}
      <g>
        <line x1={x(row.lane)} y1={0} x2={x(row.lane)} y2={18} stroke={laneColor(row.lane)} strokeWidth={2} opacity={opacity(row.lane)} pointerEvents="none" />
        <line x1={x(row.lane)} y1={0} x2={x(row.lane)} y2={18} {...hitProps(row.lane)} />
      </g>
      {row.continues && (
        <g>
          <line x1={x(row.lane)} y1={18} x2={x(row.lane)} y2={36} stroke={laneColor(row.lane)} strokeWidth={2} opacity={opacity(row.lane)} pointerEvents="none" />
          <line x1={x(row.lane)} y1={18} x2={x(row.lane)} y2={36} {...hitProps(row.lane)} />
        </g>
      )}
      {row.convergeFrom.map((lane) => {
        const d = `M ${x(lane)} 0 C ${x(lane)} 10, ${x(row.lane)} 8, ${x(row.lane)} 18`;
        return (
          <g key={`c${lane}`}>
            <path d={d} stroke={laneColor(lane)} strokeWidth={2} fill="none" opacity={opacity(lane, row.lane)} pointerEvents="none" />
            <path d={d} fill="none" {...hitProps(lane)} />
          </g>
        );
      })}
      {row.divergeTo.map((lane) => {
        const d = `M ${x(row.lane)} 18 C ${x(row.lane)} 28, ${x(lane)} 26, ${x(lane)} 36`;
        return (
          <g key={`d${lane}`}>
            <path d={d} stroke={laneColor(lane)} strokeWidth={2} fill="none" opacity={opacity(lane, row.lane)} pointerEvents="none" />
            <path d={d} fill="none" {...hitProps(lane)} />
          </g>
        );
      })}
      <circle cx={x(row.lane)} cy={18} r={5} fill={laneColor(row.lane)} opacity={opacity(row.lane)} />
      <circle
        cx={x(row.lane)}
        cy={18}
        r={8}
        fill="transparent"
        onMouseEnter={() => onHoverLane(row.lane)}
        onMouseLeave={() => onHoverLane(null)}
      />
    </svg>
  );
}

function clamp(value: number, min: number, max: number) {
  return Math.min(max, Math.max(min, value));
}

type BranchTreeNode = {
  segment: string;
  fullPath: string;
  isBranch: boolean;
  children: Map<string, BranchTreeNode>;
};

/** Groups branch names sharing a "/" prefix (e.g. "feature/x", "feature/y") into a tree. */
function buildBranchTree(branches: string[]): BranchTreeNode {
  const root: BranchTreeNode = { segment: "", fullPath: "", isBranch: false, children: new Map() };
  for (const branch of branches) {
    let node = root;
    let path = "";
    for (const part of branch.split("/")) {
      path = path ? `${path}/${part}` : part;
      let next = node.children.get(part);
      if (!next) {
        next = { segment: part, fullPath: path, isBranch: false, children: new Map() };
        node.children.set(part, next);
      }
      node = next;
    }
    node.isBranch = true;
  }
  return root;
}

type FlatRow =
  | { type: "folder"; fullPath: string; segment: string; depth: number }
  | { type: "option"; fullPath: string; depth: number };

function collectLeaves(node: BranchTreeNode): string[] {
  const out = node.isBranch ? [node.fullPath] : [];
  for (const child of node.children.values()) out.push(...collectLeaves(child));
  return out;
}

/** Flattens the tree into visible rows, respecting collapsed folders — except while searching,
 * where every folder on the path to a match is force-expanded so results stay reachable. */
function flattenBranchTree(
  node: BranchTreeNode,
  depth: number,
  collapsed: Set<string>,
  query: string,
): FlatRow[] {
  const rows: FlatRow[] = [];
  const children = [...node.children.values()].sort((a, b) => a.segment.localeCompare(b.segment));
  for (const child of children) {
    const isFolder = child.children.size > 0;
    if (query) {
      const hasMatch = collectLeaves(child).some((b) => b.toLowerCase().includes(query.toLowerCase()));
      if (!hasMatch) continue;
    }
    if (isFolder) {
      rows.push({ type: "folder", fullPath: child.fullPath, segment: child.segment, depth });
      if (query || !collapsed.has(child.fullPath)) {
        rows.push(...flattenBranchTree(child, depth + 1, collapsed, query));
        if (child.isBranch && child.fullPath.toLowerCase().includes(query.toLowerCase())) {
          rows.push({ type: "option", fullPath: child.fullPath, depth: depth + 1 });
        }
      }
    } else {
      rows.push({ type: "option", fullPath: child.fullPath, depth });
    }
  }
  return rows;
}

/** A branch picker with search-to-filter, "/"-prefix folder grouping (collapsible, preserved
 * while searching), and arrow-key navigation — a native <select> can't scroll-search or group. */
function BranchPicker({
  value,
  branches,
  onChange,
}: {
  value: string;
  branches: string[];
  onChange: (branch: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [collapsed, setCollapsed] = useState<Set<string>>(new Set());
  const [highlighted, setHighlighted] = useState(0);
  const rootRef = useRef<HTMLDivElement>(null);
  const listRef = useRef<HTMLDivElement>(null);

  const rows = flattenBranchTree(buildBranchTree(branches), 0, collapsed, query);
  const optionPaths = rows.filter((r) => r.type === "option").map((r) => r.fullPath);

  useEffect(() => {
    if (!open) return;
    function onDocMouseDown(e: MouseEvent) {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) setOpen(false);
    }
    document.addEventListener("mousedown", onDocMouseDown);
    return () => document.removeEventListener("mousedown", onDocMouseDown);
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const idx = optionPaths.indexOf(value);
    setHighlighted(idx >= 0 ? idx : 0);
    // Re-run only when the popover opens or the query changes the candidate set, not on every render.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open, query]);

  useEffect(() => {
    listRef.current?.querySelector(`[data-option-index="${highlighted}"]`)?.scrollIntoView({ block: "nearest" });
  }, [highlighted]);

  function pick(branch: string) {
    onChange(branch);
    setOpen(false);
  }

  function toggleFolder(path: string) {
    setCollapsed((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  }

  function onSearchKeyDown(e: React.KeyboardEvent) {
    if (e.key === "Escape") {
      setOpen(false);
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      setHighlighted((i) => Math.min(i + 1, optionPaths.length - 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setHighlighted((i) => Math.max(i - 1, 0));
    } else if (e.key === "Enter") {
      if (optionPaths[highlighted]) pick(optionPaths[highlighted]);
    }
  }

  let optionIndex = -1;

  return (
    <div className="searchable-select" ref={rootRef}>
      <button
        type="button"
        className={"searchable-select-trigger" + (open ? " searchable-select-trigger--open" : "")}
        onClick={() => {
          setOpen((o) => !o);
          setQuery("");
        }}
      >
        <span className="searchable-select-value">{value}</span>
        <ChevronDownIcon />
      </button>
      {open && (
        <div className="searchable-select-popover">
          <input
            className="searchable-select-search"
            autoFocus
            placeholder="Search branches…"
            value={query}
            onChange={(e) => setQuery(e.currentTarget.value)}
            onKeyDown={onSearchKeyDown}
          />
          <div className="searchable-select-list" ref={listRef}>
            {rows.length === 0 && <div className="searchable-select-empty">No matches</div>}
            {rows.map((row) => {
              if (row.type === "folder") {
                const isCollapsed = !query && collapsed.has(row.fullPath);
                return (
                  <div
                    key={row.fullPath}
                    className="searchable-select-folder"
                    style={{ paddingLeft: 4 + row.depth * 14 }}
                    onClick={() => toggleFolder(row.fullPath)}
                  >
                    <span className={"searchable-select-chevron" + (isCollapsed ? "" : " searchable-select-chevron--open")}>
                      <ChevronDownIcon />
                    </span>
                    <FolderIcon />
                    {row.segment}
                  </div>
                );
              }
              optionIndex++;
              const index = optionIndex;
              const selected = row.fullPath === value;
              return (
                <div
                  key={row.fullPath}
                  data-option-index={index}
                  className={
                    "searchable-select-option" +
                    (selected ? " searchable-select-option--selected" : "") +
                    (index === highlighted ? " searchable-select-option--highlighted" : "")
                  }
                  style={{ paddingLeft: 8 + row.depth * 14 }}
                  onMouseEnter={() => setHighlighted(index)}
                  onClick={() => pick(row.fullPath)}
                >
                  <span className="searchable-select-option-label">{row.fullPath.split("/").pop()}</span>
                  {selected && <CheckIcon />}
                </div>
              );
            })}
          </div>
        </div>
      )}
    </div>
  );
}

function RefreshIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round">
      <path d="M13.5 8a5.5 5.5 0 1 1-1.6-3.9" />
      <path d="M13.5 2.5v3.5H10" />
    </svg>
  );
}

function LayoutSidebarIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinejoin="round">
      <rect x="1.5" y="2.5" width="13" height="11" rx="1.5" />
      <path d="M5.5 2.5v11" strokeLinecap="round" />
    </svg>
  );
}

function LayoutFocusedIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinejoin="round">
      <rect x="1.5" y="2.5" width="13" height="11" rx="1.5" />
    </svg>
  );
}

function ChevronDownIcon() {
  return (
    <svg width="10" height="10" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round">
      <path d="M4 6l4 4 4-4" />
    </svg>
  );
}

function FolderIcon() {
  return (
    <svg width="12" height="12" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.3" strokeLinejoin="round">
      <path d="M2 4.5a1 1 0 0 1 1-1h3l1.3 1.5H13a1 1 0 0 1 1 1V12a1 1 0 0 1-1 1H3a1 1 0 0 1-1-1V4.5z" />
    </svg>
  );
}

function CheckIcon() {
  return (
    <svg width="11" height="11" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
      <path d="M3.5 8.5l3 3 6-7" />
    </svg>
  );
}

function CloseIcon() {
  return (
    <svg width="12" height="12" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round">
      <path d="M3 3l10 10M13 3L3 13" />
    </svg>
  );
}

function GitGraphIcon() {
  return (
    <svg width="13" height="13" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round">
      <circle cx="4" cy="3" r="1.6" fill="currentColor" stroke="none" />
      <circle cx="4" cy="13" r="1.6" fill="currentColor" stroke="none" />
      <circle cx="12" cy="8" r="1.6" fill="currentColor" stroke="none" />
      <path d="M4 4.6V11.4" />
      <path d="M4 8c0-1.6 1.4-1.6 6.4-1.6" />
    </svg>
  );
}

function FollowIcon() {
  return (
    <svg width="9" height="9" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d="M8 3v10M4 7l4-4 4 4" />
    </svg>
  );
}

function PinIcon() {
  return (
    <svg width="9" height="9" viewBox="0 0 16 16" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
      <path d="M6 2h4l-.5 5 2 2-1 1H5.5l-1 -1 2-2z" />
      <path d="M8 10v4" />
    </svg>
  );
}

/** A dropdown for a small fixed set of options, styled to match BranchPicker's trigger/popover
 * instead of a bare native <select> (which looks like unstyled browser chrome next to it). */
function SimpleSelect<T extends string>({
  value,
  options,
  onChange,
  align = "left",
}: {
  value: T;
  options: { value: T; label: string }[];
  onChange: (value: T) => void;
  align?: "left" | "right";
}) {
  const [open, setOpen] = useState(false);
  const rootRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    function onDocMouseDown(e: MouseEvent) {
      if (rootRef.current && !rootRef.current.contains(e.target as Node)) setOpen(false);
    }
    document.addEventListener("mousedown", onDocMouseDown);
    return () => document.removeEventListener("mousedown", onDocMouseDown);
  }, [open]);

  return (
    <div className="searchable-select" ref={rootRef}>
      <button
        type="button"
        className={"searchable-select-trigger" + (open ? " searchable-select-trigger--open" : "")}
        onClick={() => setOpen((o) => !o)}
      >
        <span className="searchable-select-value">{options.find((o) => o.value === value)?.label ?? value}</span>
        <ChevronDownIcon />
      </button>
      {open && (
        <div className={"searchable-select-popover" + (align === "right" ? " searchable-select-popover--right" : "")}>
          <div className="searchable-select-list">
            {options.map((opt) => (
              <div
                key={opt.value}
                className={"searchable-select-option" + (opt.value === value ? " searchable-select-option--selected" : "")}
                onClick={() => {
                  onChange(opt.value);
                  setOpen(false);
                }}
              >
                <span className="searchable-select-option-label">{opt.label}</span>
                {opt.value === value && <CheckIcon />}
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  );
}

/** A second, explicit confirmation step for destructive actions — in-app styled instead of the
 * native window.confirm(), matching the rest of the app's chrome. */
function ConfirmModal({
  title,
  message,
  confirmLabel,
  onConfirm,
  onCancel,
}: {
  title: string;
  message: string;
  confirmLabel: string;
  onConfirm: () => void;
  onCancel: () => void;
}) {
  useEffect(() => {
    function onKeyDown(e: KeyboardEvent) {
      if (e.key === "Escape") onCancel();
    }
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [onCancel]);

  return (
    <div className="modal-overlay" onClick={onCancel}>
      <div className="modal confirm-modal" onClick={(e) => e.stopPropagation()}>
        <div className="confirm-modal-title">{title}</div>
        <p className="confirm-modal-message">{message}</p>
        <div className="confirm-modal-actions">
          <button type="button" className="btn" onClick={onCancel}>
            Cancel
          </button>
          <button type="button" className="btn btn--danger" onClick={onConfirm}>
            {confirmLabel}
          </button>
        </div>
      </div>
    </div>
  );
}

type FontChoice = "system" | "pretendard";
const FONT_STORAGE_KEY = "maditor:font";

function loadFontChoice(): FontChoice {
  try {
    return localStorage.getItem(FONT_STORAGE_KEY) === "pretendard" ? "pretendard" : "system";
  } catch {
    return "system";
  }
}

type SettingsTab = "general";

/** Settings modal, opened via the gear icon or Cmd/Ctrl+, (the OS-standard preferences
 * shortcut). Tabbed on the left for when more sections show up; only "General" exists today. */
function SettingsModal({ onClose }: { onClose: () => void }) {
  const [tab, setTab] = useState<SettingsTab>("general");
  const [font, setFont] = useState<FontChoice>(loadFontChoice);

  useEffect(() => {
    if (font === "pretendard") {
      import("pretendard/dist/web/variable/pretendardvariable.css").then(() => {
        document.documentElement.dataset.font = "pretendard";
      });
    } else {
      delete document.documentElement.dataset.font;
    }
    try {
      localStorage.setItem(FONT_STORAGE_KEY, font);
    } catch {
      // best-effort
    }
  }, [font]);

  useEffect(() => {
    function onKeyDown(e: KeyboardEvent) {
      if (e.key === "Escape") onClose();
    }
    document.addEventListener("keydown", onKeyDown);
    return () => document.removeEventListener("keydown", onKeyDown);
  }, [onClose]);

  return (
    <div className="modal-overlay" onClick={onClose}>
      <div className="modal settings-modal" onClick={(e) => e.stopPropagation()}>
        <div className="settings-header">
          <span className="settings-title">Settings</span>
          <button type="button" className="icon-btn" onClick={onClose} aria-label="Close settings">
            <CloseIcon />
          </button>
        </div>
        <div className="settings-body">
          <div className="settings-tabs">
            <button
              type="button"
              className={"settings-tab" + (tab === "general" ? " settings-tab--active" : "")}
              onClick={() => setTab("general")}
            >
              General
            </button>
          </div>
          <div className="settings-content">
            {tab === "general" && (
              <div className="settings-row">
                <span className="settings-label">Font</span>
                <SimpleSelect
                  value={font}
                  onChange={setFont}
                  align="right"
                  options={[
                    { value: "system", label: "System" },
                    { value: "pretendard", label: "Pretendard" },
                  ]}
                />
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

function defaultBranchOf(branches: string[]): string {
  return branches.includes("main") ? "main" : (branches[0] ?? "main");
}

function baseBranchesStorageKey(repoPath: string) {
  return `maditor:base-branches:${repoPath}`;
}

function loadPinnedBaseBranches(repoPath: string): Record<string, string> {
  try {
    const raw = localStorage.getItem(baseBranchesStorageKey(repoPath));
    return raw ? JSON.parse(raw) : {};
  } catch {
    return {};
  }
}

function savePinnedBaseBranches(repoPath: string, map: Record<string, string>) {
  try {
    localStorage.setItem(baseBranchesStorageKey(repoPath), JSON.stringify(map));
  } catch {
    // best-effort; private browsing or storage quota issues just mean re-pinning next launch
  }
}

const PROJECTS_STORAGE_KEY = "maditor:projects";
const LAST_PROJECT_STORAGE_KEY = "maditor:last-project";

function loadProjects(): string[] {
  try {
    const raw = localStorage.getItem(PROJECTS_STORAGE_KEY);
    return raw ? JSON.parse(raw) : [];
  } catch {
    return [];
  }
}

function saveProjects(paths: string[]) {
  try {
    localStorage.setItem(PROJECTS_STORAGE_KEY, JSON.stringify(paths));
  } catch {
    // best-effort
  }
}

type ViewMode = "sidebar" | "focused";

function App() {
  const [repoPath, setRepoPath] = useState("");
  const [branches, setBranches] = useState<string[]>([]);
  // Each worktree's base branch is pinned once (at first scan) and remembered here, keyed by
  // worktree path, instead of one global selector that would redefine "changed" for every
  // worktree whenever it's touched.
  const [baseBranches, setBaseBranches] = useState<Record<string, string>>({});
  const [worktrees, setWorktrees] = useState<WorktreeInfo[]>([]);
  const [selectedWorktree, setSelectedWorktree] = useState<WorktreeInfo | null>(null);
  const [diff, setDiff] = useState<DiffResult | null>(null);
  const [selectedFile, setSelectedFile] = useState<FileDiff | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [viewMode, setViewMode] = useState<ViewMode>("sidebar");
  const [contextMenu, setContextMenu] = useState<{ x: number; y: number; worktree: WorktreeInfo } | null>(null);
  const [confirmRemove, setConfirmRemove] = useState<WorktreeInfo | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [projects, setProjects] = useState<string[]>(() => loadProjects());
  const [gitPanelOpen, setGitPanelOpen] = useState(false);
  const [gitPanelHeight, setGitPanelHeight] = useState(280);
  const [graphBranch, setGraphBranch] = useState("");
  const [followWorktree, setFollowWorktree] = useState(true);

  useEffect(() => {
    function onKeyDown(e: KeyboardEvent) {
      if ((e.metaKey || e.ctrlKey) && e.key === ",") {
        e.preventDefault();
        setSettingsOpen(true);
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  // Reopen whichever project was active last time, on launch only.
  useEffect(() => {
    const last = localStorage.getItem(LAST_PROJECT_STORAGE_KEY);
    if (!last) return;
    setProjects((prev) => {
      const next = prev.includes(last) ? prev : [...prev, last];
      saveProjects(next);
      return next;
    });
    scan(last);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // The git panel's branch tracks the selected worktree by default; a manual pick in the panel
  // (which flips followWorktree off) breaks that link until another worktree is clicked. With no
  // worktree selected (just a project), fall back to "main" (or the repo's first branch) so the
  // graph isn't stuck empty.
  useEffect(() => {
    if (!followWorktree) return;
    if (selectedWorktree?.branch) {
      setGraphBranch(selectedWorktree.branch);
    } else if (branches.length > 0) {
      setGraphBranch(branches.includes("main") ? "main" : branches[0]);
    }
  }, [followWorktree, selectedWorktree, branches]);

  const [worktreePanelWidth, setWorktreePanelWidth] = useState(248);
  const [filePanelWidth, setFilePanelWidth] = useState(260);

  async function refreshWorktrees(path: string, pins: Record<string, string>) {
    const wts = await invoke<WorktreeInfo[]>("list_worktrees", { repoPath: path, baseBranches: pins });
    setWorktrees(wts);
    return wts;
  }

  async function scan(path: string) {
    setRepoPath(path);
    setError(null);
    setDiff(null);
    setSelectedWorktree(null);
    setSelectedFile(null);
    setFollowWorktree(true);
    setGraphBranch("");
    try {
      const brs = await invoke<string[]>("list_branches", { repoPath: path });
      setBranches(brs);

      const pins = loadPinnedBaseBranches(path);
      let wts = await refreshWorktrees(path, pins);

      // Pin any newly discovered worktree to a sensible default, then refresh once more so
      // its ahead/behind reflects that pin immediately.
      let changed = false;
      for (const wt of wts) {
        if (!pins[wt.path]) {
          pins[wt.path] = defaultBranchOf(brs);
          changed = true;
        }
      }
      if (changed) wts = await refreshWorktrees(path, pins);

      setBaseBranches(pins);
      savePinnedBaseBranches(path, pins);
      try {
        localStorage.setItem(LAST_PROJECT_STORAGE_KEY, path);
      } catch {
        // best-effort
      }
    } catch (e) {
      setWorktrees([]);
      setBranches([]);
      setError(String(e));
    }
  }

  async function pickProject() {
    const dir = await open({ directory: true, multiple: false, title: "Open a git repository" });
    if (typeof dir !== "string") return;
    setProjects((prev) => {
      const next = prev.includes(dir) ? prev : [...prev, dir];
      saveProjects(next);
      return next;
    });
    scan(dir);
  }

  function rescan() {
    if (repoPath) scan(repoPath);
  }

  async function loadDiff(wt: WorktreeInfo, branch: string) {
    setDiff(null);
    setSelectedFile(null);
    setError(null);
    try {
      const result = await invoke<DiffResult>("diff_against_base", {
        worktreePath: wt.path,
        baseBranch: branch,
      });
      setDiff(result);
      setSelectedFile(result.files[0] ?? null);
    } catch (e) {
      setError(String(e));
    }
  }

  function selectWorktree(wt: WorktreeInfo) {
    setSelectedWorktree(wt);
    setFollowWorktree(true);
    const base = baseBranches[wt.path];
    if (base) loadDiff(wt, base);
  }

  async function changeWorktreeBase(wt: WorktreeInfo, branch: string) {
    const pins = { ...baseBranches, [wt.path]: branch };
    setBaseBranches(pins);
    savePinnedBaseBranches(repoPath, pins);
    setError(null);
    try {
      const wts = await refreshWorktrees(repoPath, pins);
      if (selectedWorktree?.path === wt.path) {
        const updated = wts.find((w) => w.path === wt.path) ?? wt;
        setSelectedWorktree(updated);
        loadDiff(updated, branch);
      }
    } catch (e) {
      setError(String(e));
    }
  }

  async function removeWorktree(wt: WorktreeInfo) {
    setError(null);
    try {
      await invoke("remove_worktree", { repoPath, worktreePath: wt.path });
      if (selectedWorktree?.path === wt.path) {
        setSelectedWorktree(null);
        setDiff(null);
        setSelectedFile(null);
      }
      const pins = Object.fromEntries(Object.entries(baseBranches).filter(([path]) => path !== wt.path));
      setBaseBranches(pins);
      savePinnedBaseBranches(repoPath, pins);
      await refreshWorktrees(repoPath, pins);
    } catch (e) {
      setError(String(e));
    }
  }

  const projectName = repoPath.split("/").filter(Boolean).pop() ?? "";
  const committedFiles = diff?.files.filter((f) => f.section === "committed") ?? [];
  const uncommittedFiles = diff?.files.filter((f) => f.section === "uncommitted") ?? [];
  const selectedBase = selectedWorktree ? baseBranches[selectedWorktree.path] : undefined;

  // Merges panels into a single centered message instead of several empty boxes.
  const noProject = !repoPath;
  const noWorktrees = repoPath !== "" && worktrees.length === 0;
  const noSelection = !noProject && !noWorktrees && !selectedWorktree;
  const noChanges = !!selectedWorktree && !!diff && diff.files.length === 0;
  const wholeAreaEmptyMessage = noProject ? "No project" : noWorktrees ? "No data" : null;
  const detailAreaEmptyMessage = noSelection ? "No data" : noChanges ? "No changes" : null;

  return (
    <div className="app-shell">
      <div className="topbar">
        <div className="breadcrumb">
          <span className="breadcrumb-brand">maditor</span>
          {projectName && (
            <>
              <span className="breadcrumb-sep">/</span>
              <span>{projectName}</span>
            </>
          )}
          {selectedWorktree && (
            <>
              <span className="breadcrumb-sep">/</span>
              <span>{selectedWorktree.branch ?? selectedWorktree.name}</span>
            </>
          )}
        </div>
        <div className="topbar-right">
          {repoPath && (
            <button type="button" className="icon-btn" onClick={rescan} title="Rescan" aria-label="Rescan">
              <RefreshIcon />
            </button>
          )}
          <div className="view-toggle">
            <button
              type="button"
              className={"view-toggle-btn" + (viewMode === "sidebar" ? " view-toggle-btn--active" : "")}
              onClick={() => setViewMode("sidebar")}
              title="Sidebar layout"
              aria-label="Sidebar layout"
            >
              <LayoutSidebarIcon />
            </button>
            <button
              type="button"
              className={"view-toggle-btn" + (viewMode === "focused" ? " view-toggle-btn--active" : "")}
              onClick={() => setViewMode("focused")}
              title="Focused layout"
              aria-label="Focused layout"
            >
              <LayoutFocusedIcon />
            </button>
          </div>
        </div>
        {error && <span className="topbar-error">{error}</span>}
      </div>

      {settingsOpen && <SettingsModal onClose={() => setSettingsOpen(false)} />}

      {viewMode === "sidebar" ? (
        <div className="main-row">
          <div className="rail">
            {projects.map((path) => {
              const name = path.split("/").filter(Boolean).pop() ?? path;
              return (
                <button
                  key={path}
                  type="button"
                  className={"rail-tile" + (path === repoPath ? " rail-tile--active" : "")}
                  onClick={() => scan(path)}
                  title={path}
                >
                  {name[0]?.toUpperCase()}
                </button>
              );
            })}
            <button
              type="button"
              className="rail-tile rail-tile--add"
              onClick={pickProject}
              title="Open a git repository"
            >
              +
            </button>
          </div>

          {wholeAreaEmptyMessage ? (
            <EmptyArea message={wholeAreaEmptyMessage} />
          ) : (
            <>
              <div className="panel worktree-panel" style={{ width: worktreePanelWidth }}>
                <div className="panel-header">
                  <div className="panel-title">{projectName}</div>
                  <div className="panel-subtitle">Worktrees</div>
                </div>
                <div className="panel-body">
                  {worktrees.map((wt) => (
                    <div
                      key={wt.path}
                      className={
                        "list-row" + (selectedWorktree?.path === wt.path ? " list-row--selected" : "")
                      }
                      onClick={() => selectWorktree(wt)}
                      onContextMenu={(e) => {
                        e.preventDefault();
                        if (!wt.is_main) setContextMenu({ x: e.clientX, y: e.clientY, worktree: wt });
                      }}
                    >
                      <div className="list-row-title">{wt.name}</div>
                      <div className="list-row-sub">{wt.branch ?? "(detached)"}</div>
                      <div className="list-row-base" onClick={(e) => e.stopPropagation()}>
                        <span className="list-row-base-label">base:</span>
                        <BranchPicker
                          value={baseBranches[wt.path] ?? ""}
                          branches={branches}
                          onChange={(b) => changeWorktreeBase(wt, b)}
                        />
                        {wt.ahead !== null && wt.behind !== null && (
                          <span className="list-row-status">
                            {wt.ahead === 0 && wt.behind === 0 ? "up to date" : `↑${wt.ahead} ↓${wt.behind}`}
                          </span>
                        )}
                      </div>
                    </div>
                  ))}
                </div>
              </div>

              <Resizer onResize={(dx) => setWorktreePanelWidth((w) => clamp(w + dx, 180, 420))} />

              {detailAreaEmptyMessage ? (
                <EmptyArea message={detailAreaEmptyMessage} />
              ) : (
                <>
                  <div className="panel file-panel" style={{ width: filePanelWidth }}>
                    <div className="panel-header">
                      <div className="panel-title">{selectedWorktree!.name}</div>
                      <div className="panel-subtitle">vs {selectedBase}</div>
                    </div>
                    <div className="panel-body">
                      {committedFiles.length > 0 && (
                        <div className="file-section-label">COMMITTED · {committedFiles.length}</div>
                      )}
                      {committedFiles.map((f) => (
                        <FileRow key={f.path} file={f} selected={selectedFile === f} onClick={() => setSelectedFile(f)} />
                      ))}
                      {uncommittedFiles.length > 0 && (
                        <div className="file-section-label">UNCOMMITTED · {uncommittedFiles.length}</div>
                      )}
                      {uncommittedFiles.map((f) => (
                        <FileRow key={f.path} file={f} selected={selectedFile === f} onClick={() => setSelectedFile(f)} />
                      ))}
                    </div>
                  </div>

                  <Resizer onResize={(dx) => setFilePanelWidth((w) => clamp(w + dx, 180, 480))} />

                  <div className="panel diff-panel">
                    <DiffView file={selectedFile} />
                  </div>
                </>
              )}
            </>
          )}
        </div>
      ) : (
        <div className="focused-layout">
          {wholeAreaEmptyMessage ? (
            <EmptyArea message={wholeAreaEmptyMessage} />
          ) : (
            <>
              <div className="focused-subbar">
                <select
                  className="focused-select"
                  value={repoPath}
                  onChange={(e) => scan(e.currentTarget.value)}
                >
                  {projects.map((path) => (
                    <option key={path} value={path}>
                      {path.split("/").filter(Boolean).pop() ?? path}
                    </option>
                  ))}
                </select>
                <select
                  className="focused-select"
                  value={selectedWorktree?.path ?? ""}
                  onChange={(e) => {
                    const wt = worktrees.find((w) => w.path === e.currentTarget.value);
                    if (wt) selectWorktree(wt);
                  }}
                >
                  <option value="" disabled>
                    Select worktree…
                  </option>
                  {worktrees.map((wt) => (
                    <option key={wt.path} value={wt.path}>
                      {wt.name} ({wt.branch ?? "detached"})
                    </option>
                  ))}
                </select>
                {selectedWorktree && (
                  <span className="focused-base">vs {selectedBase}</span>
                )}
                <select
                  className="focused-select"
                  value={selectedFile?.path ?? ""}
                  onChange={(e) => {
                    const f = diff?.files.find((f) => f.path === e.currentTarget.value);
                    if (f) setSelectedFile(f);
                  }}
                  disabled={!diff}
                >
                  <option value="" disabled>
                    Select file…
                  </option>
                  {committedFiles.length > 0 && (
                    <optgroup label="Committed">
                      {committedFiles.map((f) => (
                        <option key={f.path} value={f.path}>
                          {f.path}
                        </option>
                      ))}
                    </optgroup>
                  )}
                  {uncommittedFiles.length > 0 && (
                    <optgroup label="Uncommitted">
                      {uncommittedFiles.map((f) => (
                        <option key={f.path} value={f.path}>
                          {f.path}
                        </option>
                      ))}
                    </optgroup>
                  )}
                </select>
              </div>
              {detailAreaEmptyMessage ? (
                <EmptyArea message={detailAreaEmptyMessage} />
              ) : (
                <div className="panel diff-panel diff-panel--focused">
                  <DiffView file={selectedFile} />
                </div>
              )}
            </>
          )}
        </div>
      )}

      {repoPath && gitPanelOpen && (
        <GitPanel
          repoPath={repoPath}
          branches={branches}
          branch={graphBranch}
          onBranchChange={(b) => {
            setFollowWorktree(false);
            setGraphBranch(b);
          }}
          following={followWorktree}
          height={gitPanelHeight}
          onResizeHeight={(dy) => setGitPanelHeight((h) => clamp(h - dy, 160, 640))}
          onClose={() => setGitPanelOpen(false)}
        />
      )}
      {repoPath && (
        <div className="bottom-bar">
          <button
            type="button"
            className={"bottom-tab" + (gitPanelOpen ? " bottom-tab--active" : "")}
            onClick={() => setGitPanelOpen((o) => !o)}
          >
            <GitGraphIcon />
            Graph
          </button>
        </div>
      )}

      {contextMenu && (
        <>
          <div className="context-menu-overlay" onClick={() => setContextMenu(null)} />
          <div className="context-menu" style={{ left: contextMenu.x, top: contextMenu.y }}>
            <button
              type="button"
              className="context-menu-item context-menu-item--danger"
              onClick={() => {
                setConfirmRemove(contextMenu.worktree);
                setContextMenu(null);
              }}
            >
              Remove worktree…
            </button>
          </div>
        </>
      )}

      {confirmRemove && (
        <ConfirmModal
          title="Remove worktree"
          message={`Remove worktree "${confirmRemove.name}" (${confirmRemove.branch ?? "detached"})? This deletes its working directory. Uncommitted changes will be lost.`}
          confirmLabel="Remove"
          onConfirm={() => {
            const wt = confirmRemove;
            setConfirmRemove(null);
            removeWorktree(wt);
          }}
          onCancel={() => setConfirmRemove(null)}
        />
      )}
    </div>
  );
}

function EmptyArea({ message }: { message: string }) {
  return <div className="empty-area">{message}</div>;
}

/** Full-width git panel (spans under the sidebar too, like VS Code's bottom panel) — git
 * features aren't in service of the diff view, they're their own thing that happens to default
 * to whatever worktree/branch is selected. */
function GitPanel({
  repoPath,
  branches,
  branch,
  onBranchChange,
  following,
  height,
  onResizeHeight,
  onClose,
}: {
  repoPath: string;
  branches: string[];
  branch: string;
  onBranchChange: (branch: string) => void;
  following: boolean;
  height: number;
  onResizeHeight: (deltaY: number) => void;
  onClose: () => void;
}) {
  const [commits, setCommits] = useState<CommitInfo[]>([]);
  const [hasMore, setHasMore] = useState(true);
  const [loadingMore, setLoadingMore] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [hoveredLane, setHoveredLane] = useState<number | null>(null);
  const [selectedOid, setSelectedOid] = useState<string | null>(null);
  const [commitFiles, setCommitFiles] = useState<FileDiff[] | null>(null);
  const [commitFilesError, setCommitFilesError] = useState<string | null>(null);
  const [selectedCommitFile, setSelectedCommitFile] = useState<FileDiff | null>(null);
  const [graphPaneWidth, setGraphPaneWidth] = useState(460);
  const [commitFileListWidth, setCommitFileListWidth] = useState(220);
  // Synchronous in-flight guard: `loadingMore` state can't prevent a second call fired before
  // React re-renders with the state update, since both reads would still see the stale `false`.
  const fetchingRef = useRef(false);

  useEffect(() => {
    setCommits([]);
    setHasMore(true);
    setError(null);
    setSelectedOid(null);
    setCommitFiles(null);
    setSelectedCommitFile(null);
    if (!branch) return;
    fetchingRef.current = true;
    invoke<CommitInfo[]>("git_log", { repoPath, branch, skip: 0, limit: GRAPH_PAGE_SIZE })
      .then((cs) => {
        setCommits(cs);
        setHasMore(cs.length === GRAPH_PAGE_SIZE);
      })
      .catch((e) => setError(String(e)))
      .finally(() => {
        fetchingRef.current = false;
      });
  }, [repoPath, branch]);

  function loadMore() {
    if (fetchingRef.current || !hasMore || !branch) return;
    fetchingRef.current = true;
    setLoadingMore(true);
    invoke<CommitInfo[]>("git_log", { repoPath, branch, skip: commits.length, limit: GRAPH_PAGE_SIZE })
      .then((cs) => {
        setCommits((prev) => [...prev, ...cs]);
        setHasMore(cs.length === GRAPH_PAGE_SIZE);
      })
      .catch((e) => setError(String(e)))
      .finally(() => {
        fetchingRef.current = false;
        setLoadingMore(false);
      });
  }

  function onGraphScroll(e: React.UIEvent<HTMLDivElement>) {
    const el = e.currentTarget;
    if (el.scrollHeight - el.scrollTop - el.clientHeight < 200) loadMore();
  }

  function closeCommitDetail() {
    setSelectedOid(null);
    setCommitFiles(null);
    setCommitFilesError(null);
    setSelectedCommitFile(null);
  }

  function selectCommit(oid: string) {
    if (oid === selectedOid) {
      closeCommitDetail();
      return;
    }
    setSelectedOid(oid);
    setCommitFiles(null);
    setCommitFilesError(null);
    setSelectedCommitFile(null);
    invoke<FileDiff[]>("diff_commit", { repoPath, oid })
      .then((files) => {
        setCommitFiles(files);
        setSelectedCommitFile(files[0] ?? null);
      })
      .catch((e) => setCommitFilesError(String(e)));
  }

  const rows = computeGraphRows(commits);
  const selectedCommit = selectedOid ? commits.find((c) => c.oid === selectedOid) ?? null : null;

  return (
    <div className="bottom-panel" style={{ height }}>
      <VerticalResizer onResize={onResizeHeight} />
      <div className="bottom-panel-header">
        <span className="git-icon">
          <GitGraphIcon />
        </span>
        <span className="bp-title">Git</span>
        {branch && <BranchPicker value={branch} branches={branches} onChange={onBranchChange} />}
        <span className={following ? "follow-badge" : "pin-badge"}>
          {following ? <FollowIcon /> : <PinIcon />}
          {following ? "following worktree" : "pinned"}
        </span>
        <button type="button" className="icon-btn bottom-panel-close" onClick={onClose} aria-label="Close git panel">
          <CloseIcon />
        </button>
      </div>
      <div className="bottom-panel-body">
        <div className="graph-pane" style={{ width: graphPaneWidth }}>
          <div className="graph-area" onScroll={onGraphScroll}>
            {error && <p className="diff-note">{error}</p>}
            {!error && !branch && <p className="diff-note">Select a worktree to see its history.</p>}
            {rows.map((row) => (
              <div
                className={"graph-row" + (row.commit.oid === selectedOid ? " graph-row--selected" : "")}
                key={row.commit.oid}
                onClick={() => selectCommit(row.commit.oid)}
              >
                <GraphLane row={row} hoveredLane={hoveredLane} onHoverLane={setHoveredLane} />
                <div className="graph-msg">{row.commit.summary}</div>
                <div className="graph-author">
                  <span className="avatar" style={{ background: laneColor(row.lane) }} />
                  {row.commit.author_name}
                </div>
                <div className="graph-date">{formatRelativeTime(row.commit.timestamp)}</div>
                <div className="graph-hash num">{row.commit.short_oid}</div>
              </div>
            ))}
            {loadingMore && <p className="diff-note graph-loading-more">Loading more…</p>}
          </div>
          {selectedCommit && (
            <div className="commit-message-detail">
              <button
                type="button"
                className="icon-btn commit-message-close"
                onClick={closeCommitDetail}
                aria-label="Close commit detail"
              >
                <CloseIcon />
              </button>
              <div className="commit-message-summary">{selectedCommit.summary}</div>
              {selectedCommit.body.trim() && <pre className="commit-message-body">{selectedCommit.body.trim()}</pre>}
              <div className="commit-message-meta">
                {selectedCommit.author_name} &lt;{selectedCommit.author_email}&gt; ·{" "}
                {new Date(selectedCommit.timestamp * 1000).toLocaleString()} ·{" "}
                <span className="num">{selectedCommit.oid}</span>
              </div>
            </div>
          )}
        </div>

        <Resizer onResize={(dx) => setGraphPaneWidth((w) => clamp(w + dx, 300, 800))} />

        {!selectedOid ? (
          <div className="panel diff-panel">
            <p className="diff-note">Select a commit to see its changed files.</p>
          </div>
        ) : (
          <>
            <div className="panel file-panel" style={{ width: commitFileListWidth }}>
              <div className="panel-body">
                {commitFilesError && <p className="diff-note">{commitFilesError}</p>}
                {commitFiles?.length === 0 && <p className="diff-note">No file changes.</p>}
                {commitFiles?.map((f) => (
                  <FileRow
                    key={f.path}
                    file={f}
                    selected={selectedCommitFile === f}
                    onClick={() => setSelectedCommitFile(f)}
                  />
                ))}
              </div>
            </div>
            <Resizer onResize={(dx) => setCommitFileListWidth((w) => clamp(w + dx, 160, 400))} />
            <div className="panel diff-panel">
              <DiffView file={selectedCommitFile} />
            </div>
          </>
        )}
      </div>
    </div>
  );
}

function DiffView({ file }: { file: FileDiff | null }) {
  if (!file) return null;
  return (
    <>
      <div className="diff-panel-header">
        <span className="diff-panel-path">{file.path}</span>
        <span className="num diff-add">+{file.additions}</span>
        <span className="num diff-del">-{file.deletions}</span>
      </div>
      <div className="diff-panel-body">
        <SideBySideDiff file={file} />
      </div>
    </>
  );
}

function FileRow({ file, selected, onClick }: { file: FileDiff; selected: boolean; onClick: () => void }) {
  const statusLetter = file.status[0]?.toUpperCase() ?? "?";
  return (
    <div className={"file-row" + (selected ? " file-row--selected" : "")} onClick={onClick}>
      <span className={`file-status file-status--${file.status}`}>{statusLetter}</span>
      <span className="file-path">{file.path}</span>
      <span className="num diff-add">+{file.additions}</span>
      <span className="num diff-del">-{file.deletions}</span>
    </div>
  );
}

export default App;
