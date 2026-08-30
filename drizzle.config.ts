import { defineConfig } from 'drizzle-kit';

export default defineConfig({
  out: './.openai/drizzle',
  schema: './db/schema.ts',
  dialect: 'sqlite',
});
