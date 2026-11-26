import { useId } from "react";
import type { ReactNode } from "react";
import { Select as SelectRoot, SelectContent, SelectItem, SelectTrigger, SelectValue } from "./shadcn/select.tsx";
import fieldStyles from "./Field.module.css";

export interface SelectOption {
  readonly value: string;
  readonly label: string;
  readonly icon?: ReactNode;
  readonly disabled?: boolean;
}

export interface SelectProps {
  readonly label?: string;
  readonly helperText?: string;
  readonly value: string;
  readonly onChange: (value: string) => void;
  readonly options: readonly SelectOption[];
  readonly placeholder?: string;
  readonly disabled?: boolean;
  readonly panelWidth?: "trigger" | "content";
  readonly layout?: "stacked" | "inline";
  readonly "aria-label"?: string;
}

export function Select({ label, helperText, value, onChange, options, placeholder, disabled, panelWidth = "trigger", layout = "stacked", "aria-label": ariaLabel }: SelectProps) {
  const id = useId();
  const helperId = helperText == null ? undefined : id + "-helper";
  const selected = options.find(option => option.value === value);

  return (
    <div className={[fieldStyles["hb-field"], layout === "inline" && fieldStyles["hb-field--inline"]].filter(Boolean).join(" ")}>
      {label != null && <label htmlFor={id} className={fieldStyles["hb-field-label"]}>{label}</label>}
      <SelectRoot
        value={value === "" && !selected ? null : value}
        items={options}
        disabled={disabled}
        onValueChange={next => { if (next !== null && next !== value) onChange(next); }}
      >
        <SelectTrigger id={id} aria-label={ariaLabel} aria-describedby={helperId} className="w-full min-w-0">
          <SelectValue className="min-w-0" placeholder={placeholder}>
            {selected?.icon}
            <span className="truncate">{selected?.label ?? (value || placeholder)}</span>
          </SelectValue>
        </SelectTrigger>
        <SelectContent align="start" alignItemWithTrigger={false} className={panelWidth === "content" ? "w-max max-w-(--available-width) p-1" : "min-w-0 p-1"}>
          {options.map(option => (
            <SelectItem key={option.value} value={option.value} disabled={option.disabled} label={option.label}>
              {option.icon}{option.label}
            </SelectItem>
          ))}
        </SelectContent>
      </SelectRoot>
      {helperText != null && <div id={helperId} className={fieldStyles["hb-field-helper"]}>{helperText}</div>}
    </div>
  );
}
