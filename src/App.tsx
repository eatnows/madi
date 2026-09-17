import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import "./App.css";

type WorktreeInfo = {
  name: string;
  path: string;
  branch: string | null;
  head_oid: string | null;
  is_main: boolean;
};

function App() {
  const [repoPath, setRepoPath] = useState("");
  const [worktrees, setWorktrees] = useState<WorktreeInfo[]>([]);
  const [error, setError] = useState<string | null>(null);

  async function loadWorktrees() {
    setError(null);
    try {
      setWorktrees(await invoke<WorktreeInfo[]>("list_worktrees", { repoPath }));
    } catch (e) {
      setWorktrees([]);
      setError(String(e));
    }
  }

  return (
    <main className="container">
      <h1>Worktrees</h1>

      <form
        className="row"
        onSubmit={(e) => {
          e.preventDefault();
          loadWorktrees();
        }}
      >
        <input
          value={repoPath}
          onChange={(e) => setRepoPath(e.currentTarget.value)}
          placeholder="/path/to/repo"
        />
        <button type="submit">Scan</button>
      </form>

      {error && <p style={{ color: "red" }}>{error}</p>}

      <ul style={{ textAlign: "left" }}>
        {worktrees.map((wt) => (
          <li key={wt.path}>
            <strong>{wt.name}</strong>
            {wt.is_main && " (main)"} — {wt.branch ?? "(detached)"} @{" "}
            {wt.head_oid?.slice(0, 7) ?? "?"}
            <br />
            <code>{wt.path}</code>
          </li>
        ))}
      </ul>
    </main>
  );
}

export default App;
