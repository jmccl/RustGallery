document.getElementById("picId").addEventListener('dblclick', function(event) {
    document.getElementById("edit_caption").style.display = "block";
    var ic = document.getElementById("input_caption");
    ic.focus();

    var caption = document.createElement("textarea");
    caption.innerHTML = document.getElementById("caption").firstChild.textContent;
    ic.value = caption.value;
});

document.getElementById("input_caption").addEventListener("keyup", function(event) {
    if (event.key == "Enter") {
        var url = new URL(window.location.href);
        url.hash = "";
        url.pathname = url.pathname + location.hash.substr(1);
        var ic = document.getElementById("input_caption");
        var post_url = url.toString() + "?caption=" + encodeURI(ic.value) + "&crumb=" + crumb;

        hideCaptionEdit();
        fetch(post_url, {
            method: "POST"
        }).then(response => {
            if (response.status != 200) {
                response.text().then(text => alert("Unable to update caption: " + text));
            } else {
                location.reload();
            }
        });
    }
});