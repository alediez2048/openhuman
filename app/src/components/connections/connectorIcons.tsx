/**
 * Centralized icon resolution for every connector kind in the Hub.
 *
 * Composio toolkits already have a strong source of truth in
 * `composioToolkitMeta(slug).icon` (the live Composio logo badge), so that
 * stays where it lives. This module covers the *other* five mechanisms:
 * Channels, Browser Accounts, Built-in, MCP, and Generic HTTP — each one
 * gets a branded react-icons glyph wrapped in a uniform rounded badge so
 * the Hub feels uniform across sections.
 */
import { type ReactNode } from 'react';
import { FaApple, FaGlobe, FaLink, FaLinkedin, FaPuzzlePiece, FaRobot } from 'react-icons/fa';
import {
  SiDiscord,
  SiGoogle,
  SiGooglemaps,
  SiInstagram,
  SiMessenger,
  SiSlack,
  SiTelegram,
  SiTwilio,
  SiWhatsapp,
  SiX,
} from 'react-icons/si';
import { TbMessages } from 'react-icons/tb';

/** Wraps an svg/icon in the uniform rounded-square badge used by every tile. */
function Badge({ children, color }: { children: ReactNode; color?: string }) {
  return (
    <span
      className="flex h-9 w-9 items-center justify-center overflow-hidden rounded-xl bg-white dark:bg-neutral-900 shadow-sm ring-1 ring-black/5"
      style={color ? { color } : undefined}>
      <span className="flex h-[22px] w-[22px] items-center justify-center">{children}</span>
    </span>
  );
}

/** Plain text initial used when no glyph is available. */
function InitialBadge({ initial, hue }: { initial: string; hue: string }) {
  return (
    <span
      className="flex h-9 w-9 items-center justify-center rounded-xl text-white text-sm font-semibold shadow-sm ring-1 ring-black/5"
      style={{ backgroundColor: hue }}>
      {initial}
    </span>
  );
}

// ── Channels ────────────────────────────────────────────────────────

export function channelIcon(slug: string): ReactNode {
  switch (slug) {
    case 'telegram':
      return (
        <Badge color="#229ED9">
          <SiTelegram className="h-full w-full" />
        </Badge>
      );
    case 'discord':
      return (
        <Badge color="#5865F2">
          <SiDiscord className="h-full w-full" />
        </Badge>
      );
    case 'web':
      return (
        <Badge color="#4A83DD">
          <FaGlobe className="h-full w-full" />
        </Badge>
      );
    case 'imessage':
      return (
        <Badge color="#1F2937">
          <FaApple className="h-full w-full" />
        </Badge>
      );
    default:
      return <InitialBadge initial={slug.slice(0, 1).toUpperCase()} hue="#6B7280" />;
  }
}

// ── Browser accounts ───────────────────────────────────────────────

export function webviewIcon(slug: string): ReactNode {
  switch (slug) {
    case 'whatsapp':
      return (
        <Badge color="#25D366">
          <SiWhatsapp className="h-full w-full" />
        </Badge>
      );
    case 'telegram':
      return (
        <Badge color="#229ED9">
          <SiTelegram className="h-full w-full" />
        </Badge>
      );
    case 'slack':
      return (
        <Badge color="#4A154B">
          <SiSlack className="h-full w-full" />
        </Badge>
      );
    case 'discord':
      return (
        <Badge color="#5865F2">
          <SiDiscord className="h-full w-full" />
        </Badge>
      );
    case 'linkedin':
      return (
        <Badge color="#0A66C2">
          <FaLinkedin className="h-full w-full" />
        </Badge>
      );
    case 'twitter':
      return (
        <Badge color="#000000">
          <SiX className="h-full w-full" />
        </Badge>
      );
    case 'instagram':
      return (
        <Badge color="#E4405F">
          <SiInstagram className="h-full w-full" />
        </Badge>
      );
    case 'messenger':
      return (
        <Badge color="#0084FF">
          <SiMessenger className="h-full w-full" />
        </Badge>
      );
    default:
      return <InitialBadge initial={slug.slice(0, 1).toUpperCase()} hue="#6B7280" />;
  }
}

// ── Built-in integrations ──────────────────────────────────────────

export function builtinIcon(slug: string): ReactNode {
  switch (slug) {
    case 'twilio':
      return (
        <Badge color="#F22F46">
          <SiTwilio className="h-full w-full" />
        </Badge>
      );
    case 'apify':
      return <InitialBadge initial="A" hue="#97D700" />;
    case 'google_places':
      return (
        <Badge color="#34A853">
          <SiGooglemaps className="h-full w-full" />
        </Badge>
      );
    case 'parallel':
      return <InitialBadge initial="P" hue="#7C3AED" />;
    case 'seltz':
      return <InitialBadge initial="S" hue="#06B6D4" />;
    case 'stock_prices':
      return <InitialBadge initial="$" hue="#10B981" />;
    default:
      return <InitialBadge initial={slug.slice(0, 1).toUpperCase()} hue="#6B7280" />;
  }
}

// ── MCP servers ────────────────────────────────────────────────────

export function mcpIcon(slug: string): ReactNode {
  // Curated logos for the featured catalog; fall back to a generic
  // puzzle-piece badge for user-registered or unknown servers.
  switch (slug) {
    case 'linear':
      return <InitialBadge initial="L" hue="#5E6AD2" />;
    case 'notion':
      return <InitialBadge initial="N" hue="#111111" />;
    case 'github':
      return <InitialBadge initial="GH" hue="#171515" />;
    case 'gitbooks':
      return <InitialBadge initial="GB" hue="#2D9CDB" />;
    case 'filesystem':
      return <InitialBadge initial="FS" hue="#6B7280" />;
    case 'postgres':
      return <InitialBadge initial="PG" hue="#336791" />;
    case 'brave':
      return <InitialBadge initial="B" hue="#FB542B" />;
    case 'memory':
      return <InitialBadge initial="M" hue="#7C3AED" />;
    case 'google':
      return (
        <Badge>
          <SiGoogle className="h-full w-full" />
        </Badge>
      );
    default:
      return (
        <Badge color="#6B7280">
          <FaPuzzlePiece className="h-full w-full" />
        </Badge>
      );
  }
}

// ── Generic HTTP ────────────────────────────────────────────────────

export function httpIcon(templateId: string | null): ReactNode {
  // For user-saved HTTP endpoints we use a generic link badge; featured
  // templates pass their template id so we can show a service-specific
  // glyph (n8n, Zapier, Linear, …).
  switch (templateId) {
    case 'n8n':
      return <InitialBadge initial="n8" hue="#EA4B71" />;
    case 'zapier':
      return <InitialBadge initial="Z" hue="#FF4F00" />;
    case 'make':
      return <InitialBadge initial="M" hue="#6D00CC" />;
    case 'linear':
      return <InitialBadge initial="L" hue="#5E6AD2" />;
    case 'notion':
      return <InitialBadge initial="N" hue="#111111" />;
    case 'github':
      return <InitialBadge initial="GH" hue="#171515" />;
    case 'stripe':
      return <InitialBadge initial="S" hue="#635BFF" />;
    case 'webhook_site':
      return (
        <Badge color="#6B7280">
          <FaRobot className="h-full w-full" />
        </Badge>
      );
    default:
      return (
        <Badge color="#4A83DD">
          <FaLink className="h-full w-full" />
        </Badge>
      );
  }
}

// ── Section header glyph (used in empty states only) ───────────────

export function genericConnectorIcon(): ReactNode {
  return (
    <Badge color="#6B7280">
      <TbMessages className="h-full w-full" />
    </Badge>
  );
}
