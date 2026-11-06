import markOrange from "@/assets/brand/nakos-mark-orange.png";
import { cn } from "@/lib/utils";

/**
 * The Nakos N mark (orange on transparent, from the provided lockup 3).
 * Sized by height; width follows the artwork's aspect ratio.
 */
export function NakosMark({ className, alt = "Nakos" }: { className?: string; alt?: string }) {
  return <img src={markOrange} alt={alt} draggable={false} className={cn("w-auto select-none", className)} />;
}
