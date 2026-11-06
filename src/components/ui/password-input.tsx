import * as React from "react";
import { Eye, EyeOff } from "lucide-react";
import { cn } from "@/lib/utils";
import { Input } from "./input";

type Props = Omit<React.InputHTMLAttributes<HTMLInputElement>, "type">;

/**
 * A passphrase field with an eye button to show what was typed. Hidden by default, and hidden
 * again whenever the field is unmounted. While shown, the field turns off autocorrect,
 * capitalisation and spellcheck. On Android the keyboard then sees VisiblePassword for a field
 * with a password autocomplete hint, or NoSuggestion without one (checked on the emulator), not
 * a plain text field it may autocorrect and learn from.
 */
export const PasswordInput = React.forwardRef<HTMLInputElement, Props>(({ className, ...props }, ref) => {
  const [shown, setShown] = React.useState(false);
  return (
    <div className="relative">
      <Input
        ref={ref}
        {...props}
        type={shown ? "text" : "password"}
        autoCorrect="off"
        autoCapitalize="off"
        spellCheck={false}
        className={cn("pr-11", className)}
      />
      <button
        type="button"
        onClick={() => setShown((s) => !s)}
        aria-label={shown ? "Hide passphrase" : "Show passphrase"}
        aria-pressed={shown}
        data-testid="toggle-passphrase"
        className="absolute inset-y-0 right-0 flex w-10 items-center justify-center rounded-r-md text-muted-foreground hover:text-foreground focus-visible:outline-hidden focus-visible:ring-1 focus-visible:ring-[hsl(var(--ring))]"
      >
        {shown ? <EyeOff className="h-4 w-4" /> : <Eye className="h-4 w-4" />}
      </button>
    </div>
  );
});
PasswordInput.displayName = "PasswordInput";
