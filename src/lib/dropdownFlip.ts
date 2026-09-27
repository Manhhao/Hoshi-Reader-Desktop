export function dropdownFlip(node: HTMLElement) {
  function update() {
    const menu = node.querySelector<HTMLElement>(".dropdown-content");
    const menuHeight = menu?.getBoundingClientRect().height || 288;
    const rect = node.getBoundingClientRect();
    const spaceBelow = window.innerHeight - rect.bottom;
    node.classList.toggle(
      "dropdown-top",
      spaceBelow < menuHeight + 8 && rect.top > spaceBelow,
    );
  }
  node.addEventListener("focusin", update);
  return {
    destroy() {
      node.removeEventListener("focusin", update);
    },
  };
}
