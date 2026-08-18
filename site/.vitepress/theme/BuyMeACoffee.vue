<script setup lang="ts">
// The "Buy me a coffee" button, rendered as plain markup instead of through
// buymeacoffee.com's official <script src=".../button.prod.min.js"> snippet.
//
// Why not the official snippet: that script ends with
//
//     document.writeln(bmcBtnWidget(...))
//
// and `document.write`/`writeln` cannot be used here. A dynamically inserted
// or `async` script has its write() calls dropped by the browser ("A call to
// document.write() from an asynchronously-loaded external script was
// ignored"), so the button would simply never appear; a synchronous one that
// runs after parsing finishes implicitly calls document.open() and wipes the
// whole document — which, in a SPA that re-renders on every route change, is
// the page blanking itself. Neither failure mode is acceptable.
//
// So the markup below is what that script would have written: the same
// `.bmc-btn` element, the same class names, the same slug, the same ☕ and the
// same label, set in Cookie the way `data-font` asks for. The script only
// falls back to its inline coffee-cup SVG when the browser cannot render the
// ☕ emoji, which is why the emoji — not the 10 KB of SVG paths — is used here.
//
// The one thing not taken from the snippet is its colour: #FFDD00 on black is
// the only yellow this site would contain, so the button is filled with the
// oxide accent instead. See oxide.css §10 for that reasoning and the contrast
// figures.
//
// The geometry and the Cookie face live in oxide.css §10, alongside every
// other component on this site; the font is self-hosted from theme/ rather
// than pulled from fonts.googleapis.com the way the snippet does it, so the
// site keeps its "no third-party request on any page" property.
</script>

<template>
  <div class="bmc">
    <p class="bmc-label">Support Verbora</p>
    <a
      class="bmc-btn"
      href="https://buymeacoffee.com/addlayer"
      target="_blank"
      rel="noopener noreferrer"
    >
      <span class="bmc-btn-emoji" aria-hidden="true">☕</span>
      <span class="bmc-btn-text">Buy me a coffee</span>
    </a>
    <p class="bmc-note">Verbora is free and MIT-licensed.</p>
  </div>
</template>
