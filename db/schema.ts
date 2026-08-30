import { integer, sqliteTable, text } from 'drizzle-orm/sqlite-core';

export const clips = sqliteTable('clips', {
  key: text('key').primaryKey(),
  content: text('content').notNull(),
  createdAt: integer('created_at').notNull(),
});
