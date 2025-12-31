import { clsx, type ClassValue } from 'clsx';
import { twMerge } from 'tailwind-merge';

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs));
}

export function formatNumber(value: number | null | undefined, decimals = 2): string {
  if (value == null) return '-';
  return value.toLocaleString('en-US', {
    minimumFractionDigits: decimals,
    maximumFractionDigits: decimals,
  });
}

export function formatPercent(value: number | null | undefined, decimals = 2): string {
  if (value == null) return '-';
  return `${formatNumber(value * 100, decimals)}%`;
}

export function formatBps(value: number | null | undefined, decimals = 1): string {
  if (value == null) return '-';
  return `${formatNumber(value, decimals)} bps`;
}

export function formatCurrency(value: number | null | undefined, currency = 'USD'): string {
  if (value == null) return '-';
  return new Intl.NumberFormat('en-US', {
    style: 'currency',
    currency,
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  }).format(value);
}

export function formatDate(dateStr: string): string {
  const date = new Date(dateStr);
  return date.toLocaleDateString('en-US', {
    year: 'numeric',
    month: 'short',
    day: 'numeric',
  });
}

export function formatPrice(value: number | null | undefined): string {
  if (value == null) return '-';
  // For simplicity, just show decimal
  return formatNumber(value, 4);
}

export function getPriceChangeColor(change: number | null | undefined): string {
  if (change == null || change === 0) return 'text-slate-600';
  return change > 0 ? 'text-gain' : 'text-loss';
}

export function getYieldChangeColor(change: number | null | undefined): string {
  if (change == null || change === 0) return 'text-slate-600';
  // Higher yields typically mean lower prices, so reverse the color
  return change > 0 ? 'text-loss' : 'text-gain';
}
