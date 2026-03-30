// Copyright (C) 2026 AnalyseDeCircuit
// SPDX-License-Identifier: GPL-3.0-only

import * as React from "react"
import { cn } from "../../lib/utils"

export interface InputProps
  extends React.InputHTMLAttributes<HTMLInputElement> { }

const Input = React.forwardRef<HTMLInputElement, InputProps>(
  ({ className, type, ...props }, ref) => {
    return (
      <input
        type={type}
        className={cn(
          "flex h-9 w-full rounded-sm border border-theme-border bg-theme-bg-sunken px-3 py-1 text-sm shadow-none transition-[color,background-color,border-color,box-shadow] duration-150 file:border-0 file:bg-transparent file:text-sm file:font-medium placeholder:text-theme-text-muted focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-theme-accent/70 focus-visible:ring-offset-1 focus-visible:ring-offset-theme-bg focus-visible:border-theme-accent/50 disabled:cursor-not-allowed disabled:opacity-50 text-theme-text",
          className
        )}
        ref={ref}
        {...props}
      />
    )
  }
)
Input.displayName = "Input"

export { Input }
