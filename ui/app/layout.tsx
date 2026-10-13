import type { Metadata } from "next";
import "./globals.css";

export const metadata: Metadata = {
  title: "Confidant — session notes that never leave the room",
  description:
    "Speak a twenty-second recap; a small open model on your own laptop drafts the structured note, with every line traceable to what you said. No cloud, no recording, no per-note cost.",
  icons: { icon: "/favicon.ico", apple: "/apple-icon.png" },
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en">
      <head>
        {/* The daemon replaces this placeholder with the run token when it serves the page.
            A page from any other origin cannot read it, so it cannot write to the local store. */}
        <meta name="confidant-token" content="__CONFIDANT_TOKEN__" />
      </head>
      <body>{children}</body>
    </html>
  );
}
