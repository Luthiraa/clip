import type { Metadata } from 'next';
import './globals.css';

export const metadata: Metadata = {
  title: 'clip — a 20 second paste buffer',
  description: 'One short-lived clipboard for people, laptops, and agents.',
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en">
      <body>{children}</body>
    </html>
  );
}
