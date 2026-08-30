import { env } from 'cloudflare:workers';

const TTL_MS = 20_000;

type ClipRow = { content: string; created_at: number };

export async function getClip(key: string) {
  const now = Date.now();
  const row = await env.DB.prepare(
    'SELECT content, created_at FROM clips WHERE key = ?',
  ).bind(key).first<ClipRow>();

  if (!row || row.created_at + TTL_MS <= now) return null;
  return { text: row.content, expiresAt: row.created_at + TTL_MS };
}

export async function putClip(key: string, content: string) {
  const createdAt = Date.now();
  const db = env.DB;
  await db.batch([
    db.prepare('DELETE FROM clips WHERE created_at <= ?').bind(createdAt - TTL_MS),
    db.prepare(
      `INSERT INTO clips (key, content, created_at) VALUES (?, ?, ?)
       ON CONFLICT(key) DO UPDATE SET content = excluded.content, created_at = excluded.created_at`,
    ).bind(key, content, createdAt),
  ]);
  return { expiresAt: createdAt + TTL_MS };
}

export async function delClip(key: string) {
  await env.DB.prepare('DELETE FROM clips WHERE key = ?').bind(key).run();
}
