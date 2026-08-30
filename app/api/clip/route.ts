import { delClip, getClip, putClip } from '@/db/clip';

export const runtime = 'edge';

const MAX_BYTES = 256 * 1024;
const KEY_PATTERN = /^[A-Za-z0-9_-]{16,64}$/;
const CORS = {
  'access-control-allow-origin': '*',
  'access-control-allow-methods': 'GET, PUT, DELETE, OPTIONS',
  'access-control-allow-headers': 'content-type',
};

function keyFrom(request: Request) {
  const key = new URL(request.url).searchParams.get('k') ?? '';
  return KEY_PATTERN.test(key) ? key : null;
}

export async function GET(request: Request) {
  const key = keyFrom(request);
  if (!key) return new Response('invalid room key', { status: 400, headers: CORS });

  const clip = await getClip(key);
  if (!clip) {
    return new Response(null, {
      status: 204,
      headers: { ...CORS, 'cache-control': 'no-store' },
    });
  }

  return new Response(clip.text, {
    headers: {
      ...CORS,
      'cache-control': 'no-store',
      'content-type': 'text/plain;charset=UTF-8',
      'x-clip-expires': String(clip.expiresAt),
    },
  });
}

export async function PUT(request: Request) {
  const key = keyFrom(request);
  if (!key) return new Response('invalid room key', { status: 400, headers: CORS });

  const declaredSize = Number(request.headers.get('content-length') ?? 0);
  if (declaredSize > MAX_BYTES) {
    return new Response('clip is larger than 256 KiB', { status: 413, headers: CORS });
  }

  const text = await request.text();
  if (new TextEncoder().encode(text).byteLength > MAX_BYTES) {
    return new Response('clip is larger than 256 KiB', { status: 413, headers: CORS });
  }

  const clip = await putClip(key, text);
  return Response.json(
    { expiresAt: clip.expiresAt },
    { headers: { ...CORS, 'cache-control': 'no-store' } },
  );
}

export async function DELETE(request: Request) {
  const key = keyFrom(request);
  if (!key) return new Response('invalid room key', { status: 400, headers: CORS });
  await delClip(key);
  return new Response(null, { status: 204, headers: CORS });
}

export function OPTIONS() {
  return new Response(null, { status: 204, headers: CORS });
}
