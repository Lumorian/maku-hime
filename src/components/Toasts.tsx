import { useStore } from "../store";
import { cn } from "../util";
import { IconClose } from "./icons";

export function Toasts() {
  const toasts = useStore((state) => state.toasts);
  const dismiss = useStore((state) => state.dismissToast);
  return (
    <div className="toasts">
      {toasts.map((toast) => (
        <div key={toast.id} className={cn("toast", `toast-${toast.level}`)}>
          <span>{toast.message}</span>
          <button type="button" className="icon-button" onClick={() => dismiss(toast.id)}>
            <IconClose size={13} />
          </button>
        </div>
      ))}
    </div>
  );
}
