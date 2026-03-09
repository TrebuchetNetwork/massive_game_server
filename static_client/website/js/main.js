document.addEventListener('DOMContentLoaded', () => {
  const header = document.querySelector('.site-header');
  const menu = document.querySelector('[data-menu]');
  const toggle = document.querySelector('[data-menu-toggle]');

  const setScrolled = () => {
    if (!header) return;
    header.classList.toggle('is-scrolled', window.scrollY > 12);
  };

  const closeMenu = () => {
    if (!menu || !toggle) return;
    menu.classList.remove('is-open');
    toggle.setAttribute('aria-expanded', 'false');
    document.body.classList.remove('menu-open');
  };

  if (toggle && menu) {
    toggle.addEventListener('click', () => {
      const nextOpen = !menu.classList.contains('is-open');
      menu.classList.toggle('is-open', nextOpen);
      toggle.setAttribute('aria-expanded', String(nextOpen));
      document.body.classList.toggle('menu-open', nextOpen);
    });

    menu.querySelectorAll('a').forEach((link) => {
      link.addEventListener('click', closeMenu);
    });
  }

  window.addEventListener('scroll', setScrolled, { passive: true });
  setScrolled();
});
