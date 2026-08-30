'use client';

import { useCallback, useEffect, useRef, useState } from 'react';

const KEY_PATTERN = /^[A-Za-z0-9_-]{16,64}$/;

function newKey() {
  const bytes = crypto.getRandomValues(new Uint8Array(16));
  return Array.from(bytes, (byte) => byte.toString(16).padStart(2, '0')).join('');
}

export default function Clipboard() {
  const [key, setKey] = useState('');
  const [text, setText] = useState('');
  const [expiresAt, setExpiresAt] = useState(0);
  const [now, setNow] = useState(Date.now());
  const [note, setNote] = useState('opening room');
  const textRef = useRef('');
  const writeTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const writing = useRef(false);

  const setCurrentText = useCallback((value: string) => {
    textRef.current = value;
    setText(value);
  }, []);

  useEffect(() => {
    const url = new URL(window.location.href);
    let roomKey = url.searchParams.get('k') ?? '';
    if (!KEY_PATTERN.test(roomKey)) {
      roomKey = newKey();
      url.searchParams.set('k', roomKey);
      window.history.replaceState(null, '', url);
    }
    setKey(roomKey);
    setNote('empty');
  }, []);

  const read = useCallback(async () => {
    if (!key || writeTimer.current || writing.current) return;
    try {
      const response = await fetch(`/api/clip?k=${encodeURIComponent(key)}`, {
        cache: 'no-store',
      });
      if (response.status === 204) {
        setCurrentText('');
        setExpiresAt(0);
        setNote('empty');
        return;
      }
      if (!response.ok) throw new Error('read failed');
      const incoming = await response.text();
      const incomingExpiry = Number(response.headers.get('x-clip-expires'));
      if (incoming !== textRef.current) {
        setCurrentText(incoming);
        setNote('updated elsewhere');
      }
      setExpiresAt(incomingExpiry);
    } catch {
      setNote('offline');
    }
  }, [key, setCurrentText]);

  useEffect(() => {
    if (!key) return;
    void read();
    const poll = window.setInterval(() => void read(), 800);
    const clock = window.setInterval(() => setNow(Date.now()), 250);
    return () => {
      window.clearInterval(poll);
      window.clearInterval(clock);
    };
  }, [key, read]);

  async function write(value: string) {
    writing.current = true;
    try {
      const response = await fetch(`/api/clip?k=${encodeURIComponent(key)}`, {
        method: 'PUT',
        headers: { 'content-type': 'text/plain;charset=UTF-8' },
        body: value,
      });
      if (!response.ok) throw new Error('write failed');
      const result = (await response.json()) as { expiresAt: number };
      setExpiresAt(result.expiresAt);
      setNote('shared');
    } catch {
      setNote('not shared');
    } finally {
      writing.current = false;
    }
  }

  function change(value: string) {
    setCurrentText(value);
    setNote('sharing');
    if (writeTimer.current) clearTimeout(writeTimer.current);
    writeTimer.current = setTimeout(() => {
      writeTimer.current = null;
      void write(value);
    }, 180);
  }

  async function copy() {
    await navigator.clipboard.writeText(text);
    setNote('copied');
  }

  async function share() {
    if (navigator.share) {
      await navigator.share({ title: 'clip', url: window.location.href });
    } else {
      await navigator.clipboard.writeText(window.location.href);
      setNote('link copied');
    }
  }

  async function clear() {
    if (writeTimer.current) {
      clearTimeout(writeTimer.current);
      writeTimer.current = null;
    }
    setCurrentText('');
    setExpiresAt(0);
    setNote('empty');
    await fetch(`/api/clip?k=${encodeURIComponent(key)}`, { method: 'DELETE' });
  }

  const remaining = Math.max(0, Math.ceil((expiresAt - now) / 1000));
  const live = remaining > 0;
  const status = live ? `${note} · ${remaining}s` : note;

  return (
    <main className="shell">
      <header className="top">
        <h1 className="brand">clip.</h1>
        <span className="hint">one link · one buffer · twenty seconds</span>
      </header>

      <section className="buffer" aria-label="Shared clipboard">
        <textarea
          aria-label="Clipboard text"
          autoFocus
          maxLength={262144}
          onChange={(event) => change(event.target.value)}
          placeholder="Paste something."
          spellCheck={false}
          value={text}
        />
        <div className="bar">
          <div className="status" role="status" aria-live="polite">
            <span className={`dot${live ? ' live' : ''}`} />
            <span>{status}</span>
          </div>
          <div className="actions">
            <button onClick={copy} disabled={!text} type="button">copy</button>
            <button onClick={clear} disabled={!text} type="button">clear</button>
            <button onClick={share} type="button">share room</button>
          </div>
        </div>
      </section>

      <footer className="bottom">
        <span>the room key lives in this URL</span>
        <span>last write wins</span>
      </footer>
    </main>
  );
}
