(() => {
  "use strict";

  const reduceMotion = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

  // Dense nav state without a scroll listener.
  const nav = document.querySelector(".nav");
  const sentinel = document.getElementById("nav-sentinel");
  if (nav && sentinel && "IntersectionObserver" in window) {
    const navObserver = new IntersectionObserver(
      ([entry]) => {
        nav.classList.toggle("is-scrolled", !entry.isIntersecting);
      },
      { threshold: 0 }
    );
    navObserver.observe(sentinel);
  }

  // One-time reveals.
  const revealItems = document.querySelectorAll("[data-reveal]");
  if (reduceMotion || !("IntersectionObserver" in window)) {
    revealItems.forEach((item) => item.classList.add("is-visible"));
  } else {
    const revealObserver = new IntersectionObserver(
      (entries) => {
        entries.forEach((entry) => {
          if (entry.isIntersecting) {
            entry.target.classList.add("is-visible");
            revealObserver.unobserve(entry.target);
          }
        });
      },
      { rootMargin: "0px 0px -10% 0px", threshold: 0.12 }
    );
    revealItems.forEach((item) => revealObserver.observe(item));
  }

  // Mobile menu.
  const menu = document.getElementById("mobile-menu");
  const menuOpen = document.querySelector("[data-menu-open]");
  const menuClose = document.querySelectorAll("[data-menu-close]");
  let menuReturnFocus = null;

  const closeMenu = () => {
    if (!menu || !menu.open) return;
    menu.close();
  };

  if (menu && menuOpen) {
    menuOpen.addEventListener("click", () => {
      menuReturnFocus = document.activeElement;
      menu.showModal();
      menuOpen.setAttribute("aria-expanded", "true");
      const firstClose = menu.querySelector("[data-menu-close]");
      if (firstClose) firstClose.focus();
    });

    menuClose.forEach((button) => button.addEventListener("click", closeMenu));

    menu.addEventListener("click", (event) => {
      if (event.target === menu) closeMenu();
    });

    menu.addEventListener("close", () => {
      menuOpen.setAttribute("aria-expanded", "false");
      if (menuReturnFocus && typeof menuReturnFocus.focus === "function") {
        menuReturnFocus.focus();
      }
    });

    menu.querySelectorAll("a").forEach((link) => {
      link.addEventListener("click", () => menu.close());
    });
  }

  // Theme switcher.
  const themeButtons = Array.from(document.querySelectorAll("[data-theme]"));
  const themePreviews = Array.from(document.querySelectorAll("[data-theme-preview]"));
  const themeName = document.querySelector("[data-theme-name]");

  const setTheme = (id) => {
    let activeLabel = "";
    themeButtons.forEach((button) => {
      const active = button.dataset.theme === id;
      button.setAttribute("aria-pressed", String(active));
      if (active) {
        activeLabel = button.dataset.themeLabel || button.textContent.trim();
      }
    });
    themePreviews.forEach((image) => {
      const active = image.dataset.themePreview === id;
      image.classList.toggle("is-active", active);
      image.setAttribute("aria-hidden", String(!active));
    });
    if (themeName && activeLabel) themeName.textContent = activeLabel;
  };

  themeButtons.forEach((button) => {
    button.addEventListener("click", () => setTheme(button.dataset.theme));
  });

  // Install tabs.
  const tablist = document.querySelector('[role="tablist"]');
  if (tablist) {
    const tabs = Array.from(tablist.querySelectorAll('[role="tab"]'));
    const panels = tabs
      .map((tab) => document.getElementById(tab.getAttribute("aria-controls")))
      .filter(Boolean);

    const activateTab = (tab, moveFocus) => {
      tabs.forEach((item) => {
        const active = item === tab;
        item.setAttribute("aria-selected", String(active));
        item.tabIndex = active ? 0 : -1;
      });
      panels.forEach((panel) => {
        panel.hidden = panel.id !== tab.getAttribute("aria-controls");
      });
      if (moveFocus) tab.focus();
    };

    tabs.forEach((tab) => {
      tab.addEventListener("click", () => activateTab(tab, false));
    });

    tablist.addEventListener("keydown", (event) => {
      const index = tabs.indexOf(document.activeElement);
      if (index === -1) return;
      let next = null;
      if (event.key === "ArrowRight") next = tabs[(index + 1) % tabs.length];
      if (event.key === "ArrowLeft") next = tabs[(index - 1 + tabs.length) % tabs.length];
      if (event.key === "Home") next = tabs[0];
      if (event.key === "End") next = tabs[tabs.length - 1];
      if (next) {
        event.preventDefault();
        activateTab(next, true);
      }
    });
  }

  // Copy install commands.
  document.querySelectorAll("[data-copy]").forEach((button) => {
    button.addEventListener("click", async () => {
      const block = button.closest(".code-block");
      const code = block ? block.querySelector("code") : null;
      if (!code) return;
      const text = code.textContent.trim();
      const label = button.querySelector("span");
      const original = label ? label.textContent : "";
      let copied = false;

      try {
        if (navigator.clipboard && navigator.clipboard.writeText) {
          await navigator.clipboard.writeText(text);
          copied = true;
        }
      } catch (error) {
        copied = false;
      }

      if (!copied) {
        const helper = document.createElement("textarea");
        helper.value = text;
        helper.setAttribute("readonly", "");
        helper.style.position = "fixed";
        helper.style.top = "-1000px";
        helper.style.opacity = "0";
        document.body.appendChild(helper);
        helper.select();
        try {
          copied = document.execCommand("copy");
        } catch (error) {
          copied = false;
        }
        helper.remove();
      }

      if (label) {
        label.textContent = copied
          ? button.dataset.copiedLabel || "Kopyalandı"
          : button.dataset.failedLabel || "Kopyalanamadı";
      }
      button.classList.toggle("is-copied", copied);
      window.setTimeout(() => {
        button.classList.remove("is-copied");
        if (label) label.textContent = original;
      }, 2000);
    });
  });

  // Pointer spotlight on the hero stage.
  const stage = document.querySelector("[data-spotlight]");
  if (stage && !reduceMotion && window.matchMedia("(pointer: fine)").matches) {
    stage.addEventListener("pointermove", (event) => {
      const rect = stage.getBoundingClientRect();
      stage.style.setProperty("--mx", `${event.clientX - rect.left}px`);
      stage.style.setProperty("--my", `${event.clientY - rect.top}px`);
    });
  }
})();
