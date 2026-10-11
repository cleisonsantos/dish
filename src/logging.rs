//! Escrita em stderr que nunca entra em pânico.
//!
//! No Windows, builds de release usam o subsistema GUI: quando o Dish é aberto
//! pelo Explorer ou pelo menu Iniciar não existe stderr válido, e `eprintln!`
//! entraria em pânico ao falhar a escrita. Estes avisos são de diagnóstico —
//! falhar ao escrevê-los nunca pode derrubar o aplicativo.

/// Escreve um aviso de diagnóstico, ignorando erros de escrita.
pub fn warn(args: std::fmt::Arguments<'_>) {
    use std::io::Write;
    let _ = writeln!(std::io::stderr(), "{args}");
}

#[cfg(test)]
mod tests {
    #[test]
    fn warning_never_panics() {
        super::warn(format_args!("dish: teste {}", 1));
    }
}
