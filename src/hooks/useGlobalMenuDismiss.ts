import { useEffect } from "react";

type UseGlobalMenuDismissOptions = {
  closeRuleMenu: () => void;
  closeNotificationMenu: () => void;
};

export function useGlobalMenuDismiss({
  closeRuleMenu,
  closeNotificationMenu,
}: UseGlobalMenuDismissOptions) {
  useEffect(() => {
    const closeMenu = () => {
      closeRuleMenu();
      closeNotificationMenu();
    };
    window.addEventListener("click", closeMenu);
    window.addEventListener("blur", closeMenu);
    return () => {
      window.removeEventListener("click", closeMenu);
      window.removeEventListener("blur", closeMenu);
    };
  }, [closeNotificationMenu, closeRuleMenu]);
}
