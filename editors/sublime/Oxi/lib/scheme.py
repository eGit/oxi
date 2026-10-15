"""Color scheme rules that make the transcript's Markdown read like a preview.

Sublime can't hide text, so the Markdown markers (`#`, `**`, backticks, fences, table pipes)
are faded close to the background while headings, bold, italics and code get styled. The
rules only match inside `text.oxi.transcript`, so merging them into the user's color scheme
leaves every other view alone. Colors are computed from the active scheme because a rule
can't express "the foreground at 25%" portably across schemes.
"""

T = "text.oxi.transcript "


def parse(color):
    """`#rgb`, `#rrggbb` or `#rrggbbaa` → (r, g, b); None when it isn't a hex color."""
    if not isinstance(color, str) or not color.startswith("#"):
        return None
    digits = color[1:]
    if len(digits) in (3, 4):
        digits = "".join(c * 2 for c in digits[:3])
    if len(digits) < 6:
        return None
    try:
        return tuple(int(digits[i:i + 2], 16) for i in (0, 2, 4))
    except ValueError:
        return None


def blend(color, background, amount):
    """`amount` of `color` over `background`, as `#rrggbb`."""
    return "#" + "".join(
        "{:02x}".format(int(round(c * amount + b * (1.0 - amount))))
        for c, b in zip(color, background)
    )


def rules(foreground, background, accent=None, code=None):
    """Rules for a scheme with these colors (hex strings); [] when they can't be read."""
    fg, bg = parse(foreground), parse(background)
    if fg is None or bg is None:
        return []
    accent = parse(accent) or fg
    code = parse(code) or fg
    faint = blend(fg, bg, 0.13)
    soft = blend(fg, bg, 0.7)
    tint = blend(fg, bg, 0.09)
    block = blend(fg, bg, 0.05)

    def rule(name, scopes, **style):
        entry = {"name": "oxi transcript: " + name, "scope": ", ".join(T + s for s in scopes)}
        entry.update(style)
        return entry

    return [
        # Markers: faded so the text reads as rendered.
        # Schemes often style `markup.heading punctuation...` and `entity.name.section`; the
        # longer selectors here outrank them.
        rule("markers", [
            "punctuation.definition.heading",
            "markup.heading punctuation.definition.heading",
        ] + ["markup.heading.{} punctuation.definition.heading".format(n) for n in range(1, 7)] + [
            "punctuation.definition.bold",
            "punctuation.definition.italic",
            "punctuation.definition.raw.code-fence",
            "punctuation.definition.blockquote",
            "punctuation.definition.link",
            "punctuation.definition.image",
            "punctuation.definition.metadata",
            "punctuation.separator.table-cell",
            "punctuation.section.table-header",
            "punctuation.definition.table-cell-alignment",
            "meta.table.header-separator",
            "meta.code-fence.definition",
            "constant.other.language-name",
        ], foreground=faint),
        rule("headings", ["markup.heading - punctuation", "markup.heading entity.name.section"], foreground=blend(accent, bg, 1.0), font_style="bold"),
        rule("bold", ["markup.bold - punctuation"], foreground=blend(fg, bg, 1.0), font_style="bold"),
        rule("italic", ["markup.italic - punctuation"], font_style="italic"),
        rule("bold italic", ["markup.bold markup.italic - punctuation"], font_style="bold italic"),
        # Backticks take the box's color, so they read as its padding.
        rule("inline code markers", ["markup.raw.inline punctuation.definition.raw"], foreground=tint, background=tint),
        rule("inline code", ["markup.raw.inline - punctuation"], foreground=blend(code, bg, 1.0), background=tint),
        rule("code blocks", ["meta.code-fence", "markup.raw.code-fence", "markup.raw.block"], background=block),
        rule("quotes", ["markup.quote - punctuation"], foreground=soft, font_style="italic"),
        rule("link urls", ["markup.underline.link"], foreground=soft),
        rule("table header", ["meta.table.header - punctuation"], font_style="bold"),
    ]
