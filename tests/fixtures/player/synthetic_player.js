// Synthetic YouTube player.js exercising the structural patterns the extractor
// regexes target. Not a real player; just faithful to the shapes yt-dlp scrapes.

var Ix = {
    wB: function (a) { a.reverse() },
    dN: function (a, b) { var c = a[0]; a[0] = a[b % a.length]; a[b % a.length] = c },
    J7: function (a, b) { a.splice(0, b) }
};

var ada = function (a) { a = a.split(""); Ix.wB(a, 3); Ix.J7(a, 2); Ix.dN(a, 1); return a.join("") };

var bna = function (a) {
    var b = a.split("");
    b.reverse();
    b = b.map(function (c) { return String.fromCharCode((c.charCodeAt(0) - 97 + 5) % 26 + 97) });
    return b.join("")
};

var nfd = [bna];

// signature dispatch reference (what the sig regex anchors near):
// var Pq = function (a) { var c; a.set("alr", "yes"); c = a.get("v");
//   if (c && (c = ada(decodeURIComponent(c)))) a.set("signature", c);
//   if (c = a.get("n")) a.set("n", nfd[0](c)); };
