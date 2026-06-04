// Synthetic YouTube player.js exercising the structural patterns the extractor
// regexes target. Not a real player; just faithful to the shapes yt-dlp scrapes.
//
// This fixture deliberately includes decoy single-element arrays before the real
// nsig dispatch table and a statement before the sig split, so the extractor must
// anchor on the call sites (as yt-dlp does) rather than the first match.

// The player's signature timestamp (sts), threaded into the player request.
var stsConfig = { signatureTimestamp: 19834 };

// Decoy single-element arrays that appear before the real nsig table. A naive
// "first single-element array" heuristic would wrongly capture `za`/`zb` here.
var aa = [za];
var ab = [zb];

var Ix = {
    wB: function (a) { a.reverse() },
    dN: function (a, b) { var c = a[0]; a[0] = a[b % a.length]; a[b % a.length] = c },
    J7: function (a, b) { a.splice(0, b) }
};

// The real sig transform has a leading statement before the split, so a strict
// "body must start with a=a.split" heuristic would miss it. It is located via
// the call site below.
var ada = function (a) { var z = 0; a = a.split(""); Ix.wB(a, 3); Ix.J7(a, 2); Ix.dN(a, 1); return a.join("") };

var bna = function (a) {
    var b = a.split("");
    b.reverse();
    b = b.map(function (c) { return String.fromCharCode((c.charCodeAt(0) - 97 + 5) % 26 + 97) });
    return b.join("")
};

// The real nsig dispatch table, after the decoys above.
var nfd = [bna];

// Signature/n dispatch call site (uncommented — this is what the extractor
// anchors on, like yt-dlp). `c` holds the deciphered signature; the n param is
// transformed via the dispatch array `nfd`.
var Pq = function (a) {
    var c;
    a.set("alr", "yes");
    c = a.get("v");
    if (c && (c = ada(decodeURIComponent(c)))) a.set("signature", c);
    if (c = a.get("n")) a.set("n", nfd[0](c));
};
