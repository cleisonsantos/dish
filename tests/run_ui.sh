#!/usr/bin/env bash
# Roda cada smoke uma única vez em Xvfb.
#
# O retry antigo escondia flakiness e dobrava o tempo de um teste que falhava.
# O determinismo agora vem das observações de paint e do próprio teste; uma
# falha aqui é uma falha real e deve aparecer no primeiro frame.
set -u

# Aquecimento, não teste: pagina o binário e inicializa o llvmpipe uma vez para
# o primeiro smoke não pagar esse custo dentro do primeiro paint. A fonte do
# executável é a mesma dos smokes (DISH_TEST_BIN); sem ela, o release local.
# O código de saída é ignorado, inclusive o 124 do timeout.
dish_bin="${DISH_TEST_BIN:-target/release/dish}"
if [ -x "$dish_bin" ]; then
    warm_dir="$(mktemp -d)"
    XDG_CONFIG_HOME="$warm_dir/config" XDG_DATA_HOME="$warm_dir/data" \
        xvfb-run -a -s '-screen 0 1280x960x24' timeout 8 "$dish_bin" "$warm_dir" >/dev/null 2>&1 || true
    rm -rf "$warm_dir"
fi

status=0
for test in "$@"; do
    echo "::group::$test"
    if ! xvfb-run -a -s '-screen 0 1280x960x24' python3 "$test"; then
        echo "::error::$test falhou"
        status=1
    fi
    echo "::endgroup::"
done
exit "$status"
