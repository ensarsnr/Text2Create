import { useState } from "react";
import { invoke, isTauri } from "@tauri-apps/api/core";

export default function App() {
  const [name, setName] = useState("");
  const [reply, setReply] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  async function sendToCore() {
    setReply(null);
    setError(null);
    if (!isTauri()) {
      setError(
        "The Rust core is only available inside the Text2Create desktop app, not in a web browser.",
      );
      return;
    }
    try {
      // Demo command that verifies frontend-to-Rust communication.
      setReply(await invoke<string>("greet", { name }));
    } catch (err) {
      setError(String(err));
    }
  }

  return (
    <main className="container">
      <h1>Text2Create</h1>
      <p className="notice">
        Foundation prototype. No product features are implemented yet.
      </p>

      <section>
        <h2>Frontend ↔ Rust communication check</h2>
        <form
          onSubmit={(event) => {
            event.preventDefault();
            void sendToCore();
          }}
        >
          <label htmlFor="name-input">Name</label>
          <input
            id="name-input"
            value={name}
            maxLength={100}
            onChange={(event) => setName(event.currentTarget.value)}
            placeholder="Enter a name"
          />
          <button type="submit">Send to Rust core</button>
        </form>
        {reply && <p role="status">{reply}</p>}
        {error && (
          <p role="alert" className="error">
            {error}
          </p>
        )}
      </section>
    </main>
  );
}
