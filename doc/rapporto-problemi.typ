// Rapporto sui problemi trovati nel codice di risp.
//
// Separato dal manuale dell'interprete di proposito: il manuale spiega come
// il codice funziona, questo elenca dove non funziona come dovrebbe.
//
// Ogni problema e' un elemento della tabella `problemi` qui sotto: il
// riepilogo iniziale, le schede dettagliate e l'appendice delle schede
// risolte sono generati dallo stesso dato, cosi' non possono raccontare due
// storie diverse. Per aggiungerne uno basta un nuovo elemento con gli stessi
// campi e `stato: "aperto"`. Quando il problema viene corretto, l'elemento
// passa a `stato: "risolto"`, con il commit che lo risolve in `commit`, e
// tiene solo identificativo, gravita' e titolo: il resto della storia sta
// nel messaggio di quel commit.
//
// Le schede LET e INT vengono dal rapporto di rsedit, scritto quando
// l'interprete era ancora nel suo repository, e ne conservano gli
// identificativi; le DOC sono nate dalla verifica.

#import "style.typ": *

#show: preamble.with(
  title: [risp — Rapporto sui problemi del codice],
  subtitle: [Le schede dell'interprete dall'analisi di rsedit `08a853d`, verificate sulla revisione `f1ec558`],
)

#set text(lang: "it")

// ---------------------------------------------------------------------------
// Gravità e schede
// ---------------------------------------------------------------------------

#let colore-gravita = (
  alta: rgb("#b3261e"),
  media: rgb("#a35c00"),
  bassa: rgb("#5f6368"),
)

#let ordine-gravita = (alta: 0, media: 1, bassa: 2)

#let etichetta-gravita(g) = box(
  fill: colore-gravita.at(g).lighten(88%),
  stroke: 0.5pt + colore-gravita.at(g),
  inset: (x: 4pt, y: 1.5pt),
  outset: (y: 1pt),
  radius: 2pt,
)[#text(size: 7.5pt, weight: "bold", fill: colore-gravita.at(g))[#upper(g)]]

#let etichetta-verifica(v) = if v {
  text(size: 8pt, fill: accent)[verificato]
} else {
  text(size: 8pt, fill: dim)[dal codice]
}

// Codice in linea a dimensione relativa, per i testi piu' piccoli del corpo
// (le schede e il riepilogo): la regola generale lo fissa a 9 punti, che in
// un testo a 8,5 punti lo farebbero sembrare piu' grande delle parole intorno.
#let codice-relativo(body) = {
  show raw.where(block: false): it => box(
    fill: paper,
    inset: (x: 2pt, y: 0pt),
    outset: (y: 2pt),
    radius: 2pt,
    text(font: "DejaVu Sans Mono", size: 0.94em, it.text),
  )
  body
}

// Una voce della scheda: un'etichetta in grassetto e il suo testo.
#let voce(nome, corpo) = if corpo != none {
  block(above: 0.75em, below: 0.4em)[*#nome.* #corpo]
}

// La scheda di un problema aperto. Il titolo e' un'intestazione di terzo
// livello con l'identificativo come etichetta, cosi' il riepilogo puo'
// collegarvisi.
#let scheda(p) = {
  [#heading(level: 3, outlined: false, numbering: none)[#p.id — #p.titolo] #label(p.id)]
  block(
    width: 100%,
    breakable: false,
    inset: (x: 8pt, y: 6pt),
    radius: 3pt,
    fill: paper,
    stroke: (left: 2.5pt + colore-gravita.at(p.gravita)),
  )[
    #set text(size: 9pt)
    #set par(justify: false)
    #show: codice-relativo
    #grid(
      columns: (auto, 1fr),
      column-gutter: 8pt,
      row-gutter: 5pt,
      [*Gravità*], [#etichetta-gravita(p.gravita)],
      [*Dove*], [#p.dove],
      [*Verifica*], [#p.verifica],
    )
  ]
  voce("Cosa succede", p.cosa)
  if p.at("riproduzione", default: none) != none {
    block(above: 0.75em, below: 0.3em)[*Come riprodurlo.*]
    p.riproduzione
  }
  voce("Effetto", p.at("effetto", default: none))
  voce("Correzione proposta", p.at("correzione", default: none))
}

// Le aree, nell'ordine in cui compaiono. La sigla e' il prefisso degli
// identificativi.
#let aree = (
  (sigla: "LET", nome: [Lettore]),
  (sigla: "INT", nome: [Valutatore e libreria di base]),
  (sigla: "DOC", nome: [Documentazione e test]),
)

#let area-di(p) = p.id.split("-").at(0)

// ---------------------------------------------------------------------------
// I problemi
// ---------------------------------------------------------------------------

#let problemi = (
  // ------------------------------------------------------------------ LET
  (
    id: "LET-1",
    gravita: "alta",
    stato: "risolto",
    commit: "3b7cbac",
    titolo: [Un input che finisce dentro un simbolo, con una lista aperta, esaurisce la memoria],
  ),
  (
    id: "LET-2",
    gravita: "alta",
    stato: "risolto",
    commit: "90e0eb4",
    titolo: [Un token che non può cominciare un'espressione manda il lettore in panic],
  ),
  (
    id: "LET-3",
    gravita: "media",
    stato: "risolto",
    commit: "3b7cbac",
    titolo: [Un annidamento profondo esaurisce lo stack del lettore],
  ),
  (
    id: "LET-4",
    gravita: "bassa",
    verificato: true,
    stato: "aperto",
    titolo: [`.9`, `1e5` e `+5` sono letti come simboli],
    breve: [`.9`, `1e5`, `+5` letti come simboli],
    dove: [`src/parser.rs` (`Parser::next_token`: lo stato `InDotStart`, con l'intervallo `'0'..'9'` che esclude il 9, e gli stati `InNumber` e `InNumberAfterDot`, senza esponente né segno `+`), `src/base/strings.rs` (`primitive_string_to_number`)],
    verifica: [Riprodotto sulla revisione `f1ec558`: `.9`, `1e5` e `+5` sono letti come simboli, `.5` come 0,5, e `2.5e-3` dà `NumberParseError`.],
    cosa: [Dopo un punto iniziale il lessico accetta solo le cifre da 0 a 8, quindi `.5` è un numero e `.9` un simbolo. La notazione esponenziale non esiste: `1e5` e `1e300` sono simboli, e valutarli dà `UnboundVariable`; `2.5e-3` è un errore di lettura. Anche `+5` è un simbolo. `string-to-number`, che usa `f64::from_str`, accetta invece `"1e5"`, `"inf"` e `"NaN"` (#link(<INT-17>)[INT-17]).],
    correzione: [`'0'..='9'`; esponente e segno `+` nello stato dei numeri; stesse regole per il lettore e per `string-to-number`.],
  ),
  (
    id: "LET-5",
    gravita: "bassa",
    verificato: true,
    stato: "aperto",
    titolo: [Stranezze del lessico dei simboli],
    breve: [`\r`, `"` e `,` dentro i simboli; `#'f` e `?a` simboli],
    dove: [`src/parser.rs` (`Parser::next_token`: lo stato `InSymbol`, con `'\r' => {}`, e il caso generale dello stato `Default`)],
    verifica: [Riprodotto sulla revisione `f1ec558`: `foo\rbar` si legge `foobar`, `foo"bar"` e `a,b` sono un simbolo ciascuno, `#'f` e `?a` sono simboli.],
    cosa: [Dentro un simbolo un `\r` viene saltato senza chiuderlo (`foo\rbar` diventa `foobar`); `"` e `,` ne fanno parte (`foo"bar"` è un unico simbolo, così come `a,b`). `#'f` e `?a` sono simboli e non la funzione `f` o il carattere `a` come in Emacs Lisp. Il manuale dell'interprete dice ora che non c'è un tipo carattere né `?a`; per `#'` non c'è una decisione.],
    correzione: [Trattare `\r` come spazio, chiudere il simbolo su `"`, `'`, #raw("`") e `,`; decidere se `#'` va supportato.],
  ),
  (
    id: "LET-6",
    gravita: "bassa",
    verificato: true,
    stato: "aperto",
    titolo: [Un `todo!()` nel lettore e uno stato sporco dopo un numero non valido],
    breve: [`todo!()` e stato sporco dopo `NumberParseError`],
    dove: [`src/parser.rs` (`Parser::next_token`: il `todo!` del ramo `Default` alla fine dell'input, e i ritorni di `NumberParseError`)],
    verifica: [Riprodotto sulla revisione `f1ec558`: su `1.2.3 foo` la prima chiamata a `next` dà `NumberParseError("1.2.")` e la seconda `NumberParseError("1.2..")` invece di `foo`. Il `todo!` dalla lettura del codice.],
    cosa: [Il ramo `Default` del codice dopo il ciclo chiama `todo!()` se il token non è vuoto; oggi è irraggiungibile, perché ogni passaggio a `Default` svuota prima il token. Dopo un `NumberParseError` lo stato e il token non vengono ripristinati, quindi una nuova chiamata a `next()` sullo stesso `Parser` riparte da uno stato sporco. La documentazione di `Parser::next` dice ora che dopo un errore la ripresa non è garantita, e tutti i chiamanti, in risp e in rsedit, creano un `Parser` nuovo e si fermano al primo errore: il difetto resta latente.],
    correzione: [Sostituire il `todo!` con un errore; azzerare token e stato prima di restituire ogni errore.],
  ),
  // ------------------------------------------------------------------ INT
  (
    id: "INT-1",
    gravita: "alta",
    stato: "risolto",
    commit: "b2d000b",
    titolo: [Un ciclo che contiene `unwind-protect` non esaurisce mai il carburante],
  ),
  (
    id: "INT-2",
    gravita: "alta",
    verificato: true,
    stato: "aperto",
    titolo: [Nessun limite alla profondità della ricorsione: lo stack si esaurisce e il processo termina],
    breve: [Ricorsione profonda: stack overflow e chiusura del processo],
    dove: [`src/eval.rs` (`eval` → `eval_step` → `eval_step_permitted` → `eval_special_form_or_call_step` → `eval_function_call_step`, che valuta gli argomenti con un `eval` annidato; il ramo `"spawn"`, che crea il thread con `std::thread::spawn`)],
    verifica: [Riprodotto sulla revisione `f1ec558`: con la `f` qui sotto, `(condition-case nil (f 100000) (error 'caught))` chiude il processo per stack overflow invece di restituire `caught` (build di release, su un thread di test con 2 MB di stack). Le soglie dell'analisi di rsedit (in debug tra 200 e 400 livelli, in release tra 5 000 e 6 000 sul thread principale, con 8 MB di stack) non sono state misurate di nuovo.],
    cosa: [Il trampolino elimina la crescita dello stack solo per le chiamate in coda. Una chiamata in posizione di argomento discende nello stack Rust, e non esiste un contatore di profondità.],
    riproduzione: ```lisp
(defun f (n) (if (= n 0) 0 (+ 1 (f (- n 1)))))
(condition-case nil (f 100000) (error 'caught))   ; stack overflow: il processo termina
```,
    effetto: [Un errore banale in una funzione ricorsiva scritta dall'utente chiude senza un messaggio il programma che ospita l'interprete: in rsedit l'editor, senza salvare. Uno stack overflow non è un errore Lisp, quindi nessun `condition-case` lo intercetta. Emacs, con `max-lisp-eval-depth`, solleva invece un errore.],
    correzione: [Un contatore di profondità per thread, come quello del carburante, incrementato all'ingresso di una chiamata di funzione, con un errore oltre un limite tarato sul thread con meno stack. In aggiunta, i thread di `spawn` possono essere creati con `std::thread::Builder::stack_size`; in rsedit, lo stesso vale per i thread del WorkManager (`start_thread`), che hanno anch'essi 2 MB.],
  ),
  (
    id: "INT-3",
    gravita: "media",
    verificato: true,
    stato: "aperto",
    titolo: [`setq` di una variabile mai definita crea una variabile locale che sparisce],
    breve: [`setq` su una variabile non definita dentro una funzione o un `let` crea una locale],
    dove: [`src/eval.rs` (`eval_special_form_or_call_step`, il ramo `"setq"`: `update_variable`, altrimenti `set_variable` nell'ambiente corrente), `src/environment.rs` (`Env::update_variable`, `Env::set_variable`)],
    verifica: [Riprodotto sulla revisione `f1ec558`: dopo `(defun g () (setq zz 5))` e `(g)`, leggere `zz` dà `unbound-variable`.],
    cosa: [`setq` cerca la variabile lungo la catena degli ambienti. Se non la trova, la crea nell'ambiente *corrente*, cioè in quello della funzione, del `let` o della fiber che sta eseguendo, e non nell'ambiente globale. Quando la funzione ritorna, la variabile sparisce. In Emacs Lisp un `setq` su una variabile non legata imposta il valore globale. Il manuale dell'interprete lo descrive ora tra le differenze da Emacs.],
    riproduzione: ```lisp
(defun conta () (setq contatore 1))
(conta)
contatore               ; => UnboundVariable
```,
    effetto: [Una configurazione o un modulo che fanno `setq` di uno stato globale dentro una funzione, senza un `defvar` precedente, perdono quello stato in silenzio. In rsedit, da `4506432`, `M-:` e `eval-file` valutano nell'ambiente globale, quindi i loro `setq` al livello più esterno restano; dentro un `defun` o un `let` la variabile sparisce ancora. `add-to-list` ha un difetto simile per conto suo (#link(<INT-15>)[INT-15]).],
    correzione: [Se la variabile non esiste in nessun ambiente, crearla nell'ambiente radice, come fa Emacs. Se invece la semantica attuale è voluta, va documentata in `setq` e va reso disponibile un modo esplicito (`set-default` o `defvar`) da usare dentro le funzioni.],
  ),
  (
    id: "INT-4",
    gravita: "media",
    verificato: true,
    stato: "aperto",
    titolo: [Annidamenti profondi esauriscono lo stack anche fuori dal lettore],
    breve: [Annidamento profondo in valutazione, rilascio, `equal` e stampa: stack overflow],
    dove: [`src/types.rs` (`impl Drop for ConsCell`, iterativo solo lungo il `cdr`), `src/lispexp.rs` (`impl PartialEq for LispExp`, ricorsivo sul `car`, e `impl Debug`), `src/utils.rs` (`data_to_form`), `src/eval.rs` (`eval`, `eval_backquote`)],
    verifica: [Riprodotto sulla revisione `f1ec558`: `(let ((x nil)) (dotimes (i 300000) (setq x (list x))) 'built)` chiude il processo per stack overflow quando il `let` rilascia `x`. Il resto dalla lettura del codice.],
    cosa: [Il rilascio di una lista e il confronto con `equal` scorrono la catena dei `cdr` in modo iterativo, ma scendono nei `car` per ricorsione. La valutazione di forme annidate e la stampa sono ricorsive.],
    riproduzione: ```lisp
(let ((x nil))
  (dotimes (i 300000) (setq x (list x)))
  'built)                ; stack overflow nel rilascio di x
```,
    effetto: [Abort del processo. Il lettore ora rifiuta più di 256 livelli di annidamento (#link(<LET-3>)[LET-3], risolta), quindi una struttura così profonda non può venire da un testo: deve costruirla un programma, ma basta un ciclo come quello sopra.],
    correzione: [Rilascio, `equal` e stampa con una pila esplicita; per la valutazione vale la correzione di #link(<INT-2>)[INT-2].],
  ),
  (
    id: "INT-5",
    gravita: "media",
    stato: "risolto",
    commit: "90e0eb4",
    titolo: [`dotimes` con il corpo vuoto non consuma carburante],
  ),
  (
    id: "INT-6",
    gravita: "media",
    verificato: true,
    stato: "aperto",
    titolo: [Le chiusure che si riferiscono a sé stesse non vengono mai liberate],
    breve: [Cicli di `Arc` tra chiusure e ambienti: memoria persa],
    dove: [`src/types.rs` (`Lambda`, che possiede il suo ambiente con un `Arc`), `src/environment.rs` (`Env`), `src/eval.rs` (i rami `"lambda"`, `"defun"`, `"defmacro"` e `"fiber"`, che catturano l'ambiente)],
    verifica: [Riprodotto sulla revisione `f1ec558`: `(dotimes (i 300000) (let ((f nil)) (setq f (lambda () 1)) nil))` fa crescere la memoria residente di quasi 200 MB, circa 670 byte per giro; lo stesso ciclo con `(funcall (lambda () 1))` non la fa crescere.],
    cosa: [Una lambda tiene in vita l'ambiente in cui è nata. Se quell'ambiente contiene a sua volta la lambda, si forma un ciclo di contatori di riferimenti che non scende mai a zero. Succede quando una lambda viene assegnata a una variabile del `let` che la crea, quando una callback è salvata nel `let` che la definisce, o con un `defun` dentro una funzione. Non esiste un garbage collector che rompa questi cicli.],
    effetto: [Ogni ripetizione di un idioma comune perde memoria. In una sessione lunga, con hook e callback che creano chiusure, la memoria cresce senza limite.],
    correzione: [Un raccoglitore di cicli, per esempio un mark-and-sweep periodico degli ambienti raggiungibili dall'ambiente globale e dalle pile delle fiber. Nel frattempo, documentare l'idioma da evitare.],
  ),
  (
    id: "INT-7",
    gravita: "media",
    verificato: true,
    stato: "aperto",
    titolo: [`eq` è falso per una lambda, un atomo, una fiber o una primitiva confrontati con sé stessi],
    breve: [`(eq x x)` falso per lambda, atomi, fiber e primitive],
    dove: [`src/base/predicates.rs` (`primitive_eq_impl`, dietro `eq` ed `eql`, con rami solo per numeri, simboli, `nil`, forme, cons, vettori, mappe e stringhe)],
    verifica: [Riprodotto sulla revisione `f1ec558`: per una lambda e per un atomo `(eq x x)` dà `nil` ed `(equal x x)` dà `t`; per una fiber anche `equal` dà `nil` (#link(<INT-8>)[INT-8]).],
    cosa: [Per i tipi senza un ramo dedicato `eq` cade nel caso generico e restituisce `nil`, anche quando i due argomenti sono lo stesso oggetto. Non ha un ramo nemmeno la variante `ThreadHandle`, aggiunta in `f1ec558`, che oggi però nessun codice crea (#link(<INT-18>)[INT-18]).],
    effetto: [Viene violata la legge più elementare dell'identità. Il codice che cerca una funzione o un atomo per identità non la trova.],
    correzione: [Un ramo `Arc::ptr_eq` per `Lambda`, `Atom`, `Fiber`, `Primitive` e `ThreadHandle`.],
  ),
  (
    id: "INT-8",
    gravita: "media",
    verificato: true,
    stato: "aperto",
    titolo: [Una fiber non è uguale nemmeno a sé stessa],
    breve: [Una fiber non è `eq` né `equal` a sé stessa],
    dove: [`src/types.rs` (`impl PartialEq for SharedFiber`: `std::ptr::eq(self, other)` confronta gli indirizzi dei due *involucri*, non l'`Arc` condiviso)],
    verifica: [Riprodotto sulla revisione `f1ec558`: `(setq f (fiber (yield 1)))` e poi `(list (eq f f) (equal f f) (memq f (list f)))` dà `(nil nil nil)`.],
    cosa: [Ogni volta che una fiber viene letta da una variabile se ne clona l'involucro, cioè l'`Arc`. Due cloni hanno indirizzi diversi, quindi il confronto per indirizzo dell'involucro è sempre falso.],
    effetto: [Una fiber non si trova in nessuna lista: `memq`, `member`, `assoc`, `assq` e `remove-from-list` falliscono, quindi un registro di fiber non può toglierne una.],
    correzione: [`Arc::ptr_eq(&self.0, &other.0)`.],
  ),
  (
    id: "INT-9",
    gravita: "media",
    verificato: true,
    stato: "aperto",
    titolo: [`resume` di una fiber che sta già girando la uccide],
    breve: [`resume` di una fiber in esecuzione la marca come finita],
    dove: [`src/base/fibers.rs` (`primitive_resume`), `src/types.rs` (`FiberState`, che ha solo `pending` e `is_done`)],
    verifica: [Riprodotto sulla revisione `f1ec558`: con `(setq f (fiber (resume f) (yield 1) 2))`, `(list (resume f) (fiber-done-p f) (resume f))` dà `(1 t nil)` invece di `(1 nil 2)`.],
    cosa: [Mentre una fiber gira, `resume` le toglie la pila dei frame, così un secondo `resume` non può eseguirla due volte. Ma il secondo `resume` trova la pila vuota e la marca come finita (`is_done = true`). Quando la fiber in corso fa `yield`, la pila viene salvata, però `is_done` resta vero e la fiber non riparte più. Il commento nel codice dice che togliere la continuazione impedisce a due `resume` di eseguirla due volte, e la documentazione di `FiberState::pending` che una pila vuota può voler dire che la fiber sta girando: nessuno dei due dice che il secondo `resume` la dichiara finita. Nella revisione `b2d000b`, quella che usa rsedit, il commento diceva che il secondo `resume` «non fa nulla».],
    effetto: [In rsedit, un comando che fa `resume` della fiber di un lavoro Lisp mentre il WorkManager la sta eseguendo la fa finire in silenzio: `LispJob::step` legge `fiber_is_done` e chiude il lavoro.],
    correzione: [Uno stato esplicito «in esecuzione», distinto da «finita». Un `resume` su una fiber in esecuzione restituisce `nil` o un errore senza toccarla.],
  ),
  (
    id: "INT-10",
    gravita: "media",
    verificato: true,
    stato: "aperto",
    titolo: [Un `yield` dentro `condition-case` non sospende la fiber: diventa un errore e il gestore lo intercetta],
    breve: [`yield` dentro `condition-case`: catturato come errore, la fiber non si sospende],
    dove: [`src/eval.rs` (`eval_special_form_or_call_step`, il ramo `"condition-case"`, che valuta la forma protetta con un `eval` senza il permesso di `yield`), `src/utils.rs` (`error_symbol`, `condition_matches`)],
    verifica: [Riprodotto sulla revisione `f1ec558`: `(fiber (condition-case e (yield 1) (error (setq got e) 'h)) 3)` restituisce 3 al primo `resume`, la fiber risulta finita e il gestore riceve `(error "YieldNotAllowed")`.],
    cosa: [Un `yield` è lecito solo dove il valutatore può registrare un punto di ripresa: un'istruzione del corpo o una forma di un `while`. Il corpo di `condition-case` non lo è, quindi il `yield` produce `YieldNotAllowed`. Il gestore generico `error` lo tratta come un errore qualsiasi.],
    effetto: [Una fiber che protegge con `condition-case` un passo che contiene un `yield` non si sospende mai. Il gestore d'errore parte e la fiber prosegue come se il passo fosse fallito. Il codice sembra corretto e si comporta in un altro modo.],
    correzione: [Fare in modo che `condition_matches` non intercetti mai `YieldNotAllowed`, così l'errore arriva all'autore. Oppure aggiungere un tipo di frame per `condition-case`, così il `yield` diventa lecito anche lì. Il test `a_yield_inside_a_condition_case_is_refused` fissa il comportamento di oggi e va cambiato con la correzione.],
  ),
  (
    id: "INT-11",
    gravita: "media",
    verificato: true,
    stato: "aperto",
    titolo: [Il backquote ignora la coda puntata, le mappe e l'annidamento],
    breve: [Backquote: coda puntata, mappe e annidamento non elaborati],
    dove: [`src/eval.rs` (`eval_backquote`, che restituisce le `Cons` e le mappe senza modifiche)],
    verifica: [Riprodotto sulla revisione `f1ec558`: con `b` uguale a 2, #raw("`(a . ,b)") dà `(a . (unquote b))` invece di `(a . 2)`; #raw("`{k ,b}") dà `{k (unquote b)}`; un backquote annidato dà `UndefinedFunction("unquote")`.],
    cosa: [Il lettore produce una `Cons` per una lista puntata e una mappa per `{…}`, e `eval_backquote` elabora solo le forme proprie e i vettori. Un livello di annidamento è documentato come limite, ma la coda puntata e le mappe no.],
    effetto: [Le macro che costruiscono liste puntate o mappe con il backquote producono strutture che contengono ancora `unquote`, e falliscono più tardi, lontano dalla causa.],
    correzione: [Elaborare la coda di una `Cons` e i valori di una mappa nello stesso modo degli elementi di una lista; per l'annidamento, contare i livelli come in Emacs.],
  ),
  (
    id: "INT-12",
    gravita: "media",
    verificato: true,
    stato: "aperto",
    titolo: [`format` interpreta le sequenze con backslash della stringa di formato],
    breve: [`format` interpreta `\b`, `\U`, `\.`… a tempo di esecuzione],
    dove: [`src/base/strings.rs` (`primitive_format`)],
    verifica: [Riprodotto sulla revisione `f1ec558`: `(format "\\b%s\\b" "w")` e `(format "C:\\Users\\%s" "me")` falliscono con «Wrong escape character». Che la sequenza `\n` scritta come due caratteri diventi un a capo viene dalla lettura del codice.],
    cosa: [Il lettore ha già trasformato le sequenze di escape della stringa letterale. `format` le interpreta una seconda volta e rifiuta quelle che non conosce.],
    effetto: [Non si può costruire con `format` un'espressione regolare con `\b` o `\s`, né un percorso Windows. Il testo che arriva da una variabile viene alterato.],
    correzione: [Togliere l'interpretazione delle sequenze da `format`, che è compito del lettore.],
  ),
  (
    id: "INT-13",
    gravita: "media",
    verificato: true,
    stato: "aperto",
    titolo: [`make-string` non ha un limite e non consuma carburante],
    breve: [`make-string` senza limite né carburante: un conteggio grande riempie la memoria],
    dove: [`src/base/strings.rs` (`primitive_make_string`)],
    verifica: [Riprodotto sulla revisione `f1ec558`: `(make-string (* 10000000 1000000) "a")` e un conteggio di 10#super[23] danno l'errore `runtime-error` «make-string: unable to allocate requested string»; nell'analisi di rsedit il primo terminava il processo (uscita 134) e il secondo andava in panic («capacity overflow»). Che un conteggio concesso dall'allocatore riempia la memoria viene dalla lettura del codice.],
    cosa: [Da `90e0eb4` la lunghezza viene controllata contro l'overflow e la memoria è chiesta con `try_reserve_exact`, quindi una richiesta impossibile è un errore Lisp. Manca ancora un limite esplicito: una richiesta che l'allocatore concede, fino alla memoria fisica più lo swap, viene allocata e riempita un carattere alla volta, senza consumare carburante.],
    effetto: [`(make-string 2000000000 "a")` occupa 2 GB e due miliardi di giri che il carburante non ferma; con poca memoria libera il processo può essere ucciso dal sistema. Lo stesso schema si ritrova nelle primitive dell'editor (CMD-3, nel rapporto di rsedit).],
    correzione: [Un limite esplicito (per esempio la dimensione massima di un buffer) con un errore `args-out-of-range`, e un consumo di carburante proporzionale alla lunghezza.],
  ),
  (
    id: "INT-14",
    gravita: "bassa",
    verificato: true,
    stato: "aperto",
    titolo: [I numeri interi oltre 2#super[63] vengono stampati saturati],
    breve: [Interi oltre 2#super[63] stampati come 9223372036854775807],
    dove: [`src/base/mod.rs` (`format_number`: `n as i64`, usata da `lisp_display`), `src/base/strings.rs` (`%d` di `primitive_format`)],
    verifica: [Riprodotto sulla revisione `f1ec558`: `(number-to-string 18446744073709551616)` dà `"9223372036854775807"`.],
    cosa: [Un numero intero viene stampato convertendolo a `i64`, conversione che satura. `%d` di `format` satura allo stesso modo.],
    correzione: [Stampare come intero solo se `n.abs() < 2^53`, cioè quando la rappresentazione è esatta; altrimenti usare la forma decimale di `f64`.],
  ),
  (
    id: "INT-15",
    gravita: "bassa",
    verificato: true,
    stato: "aperto",
    titolo: [`add-to-list` e funzioni affini: ambiente sbagliato, terzo argomento, costo quadratico],
    breve: [`add-to-list`: ambiente del chiamante, terzo argomento, O(n·m)],
    dove: [`src/base/lists.rs` (`primitive_add_to_list`, `primitive_append_to_list`, `primitive_remove_from_list`)],
    verifica: [Riprodotto sulla revisione `f1ec558`: dopo `(setq l '(a))` e `(defun h () (add-to-list 'l 'b))`, `(h)` lascia `l` uguale a `(a)`. Gli altri punti dalla lettura del codice.],
    cosa: [La lista viene letta con `get_variable`, che risale la catena degli ambienti, e riscritta con `set_variable` nell'ambiente del chiamante, anche quando esiste quella globale: è un difetto a sé, che correggere `setq` (#link(<INT-3>)[INT-3]) non risolve. Il terzo argomento non ha il significato di APPEND che ha in Emacs: viene aggiunto come elemento. Con più elementi, questi sono aggiunti in ordine inverso, senza controllare i duplicati tra gli argomenti stessi. Il costo è O(n·m) e non consuma carburante. Il messaggio d'errore di `remove-from-list` dice «add-to-list».],
    correzione: [Aggiornare la variabile dove è definita (`update_variable`); allineare la firma a quella di Emacs, `(add-to-list SYMBOL ELEMENT &optional APPEND)`.],
  ),
  (
    id: "INT-16",
    gravita: "bassa",
    verificato: true,
    stato: "aperto",
    titolo: [`memq` e `assq` confrontano con `equal`; `member` restituisce una copia],
    breve: [`memq`/`assq` usano `equal`; `member` copia la coda],
    dove: [`src/base/lists.rs` (`primitive_memq`, `primitive_assq` e le loro docstring), `src/base/mod.rs` (`find_member`, `find_assoc`)],
    verifica: [Riprodotto sulla revisione `f1ec558`: `(memq "a" '("a"))` dà `("a")` mentre `(eq "a" "a")` dà `nil`. Il resto dalla lettura del codice.],
    cosa: [`memq` e `assq` usano lo stesso confronto strutturale di `member` e `assoc`, e la loro docstring dice ancora «eq is structural here», mentre `eq` ora confronta per identità. `member` e `memq` restituiscono una *copia* della coda (`proper_list(list[pos..].to_vec())`), non la coda condivisa: `(eq (member 2 l) (cdr l))` è falso e il costo è O(n).],
    correzione: [Confronto con `eq` per `memq` e `assq`, docstring aggiornate, restituzione della coda condivisa scorrendo le `Cons`.],
  ),
  (
    id: "INT-17",
    gravita: "bassa",
    verificato: true,
    stato: "aperto",
    titolo: [`string-to-number` accetta `inf` e `NaN`; `substring` oltre la fine restituisce la stringa vuota],
    breve: [`string-to-number` accetta inf/NaN; `substring` oltre la fine dà `""`],
    dove: [`src/base/strings.rs` (`primitive_string_to_number`, `primitive_substring`)],
    verifica: [Riprodotto sulla revisione `f1ec558`: `string-to-number` dà `inf`, `NaN` e 100000 per `"inf"`, `"NaN"` e `"1e5"`; `(substring "abc" 5)` dà `""`.],
    cosa: [`string-to-number` usa `f64::from_str`, che accetta `"inf"`, `"NaN"` e `"1e5"`, mentre il lettore non legge nessuna delle tre come un numero (#link(<LET-4>)[LET-4]). `substring` con un inizio oltre la fine della stringa restituisce `""` invece di un errore `args-out-of-range`; un inizio maggiore della fine richiesta è un errore, ma `runtime-error`.],
    correzione: [Un unico analizzatore di numeri, condiviso dal lettore e da `string-to-number`; un errore per gli indici fuori dai limiti.],
  ),
  (
    id: "INT-18",
    gravita: "bassa",
    verificato: true,
    stato: "aperto",
    titolo: [`spawn` restituisce una forma vuota che è `eq` a `nil` ma non `equal`],
    breve: [`spawn` restituisce una `Form` vuota],
    dove: [`src/eval.rs` (`eval_special_form_or_call_step`, il ramo `"spawn"`)],
    verifica: [Riprodotto sulla revisione `f1ec558`: `(list (eq (spawn (lambda () 1)) nil) (equal (spawn (lambda () 1)) nil) (null (spawn (lambda () 1))))` dà `(t nil t)`.],
    cosa: [Il valore restituito è `LispExp::form(vec![])`, cioè una lista vuota, invece del `nil` canonico. `is_nil` la considera `nil`, il confronto strutturale no. Il commit `f1ec558`, «spawn returns a ThreadHandle value», aggiunge la variante `LispExp::ThreadHandle` ma non la usa: `spawn` restituisce ancora la forma vuota.],
    correzione: [Restituire `LispExp::nil()`, oppure il `ThreadHandle` previsto, con i suoi rami in `eq` ed `equal` (#link(<INT-7>)[INT-7]).],
  ),
  (
    id: "INT-19",
    gravita: "bassa",
    verificato: true,
    stato: "aperto",
    titolo: [Il controllo all'avvio lascia tre variabili globali],
    breve: [`bootstrap_vm` lascia `verify-math`, `verify-fiber`, `fiber-step`],
    dove: [`src/handshake.rs` (`bootstrap_vm`)],
    verifica: [Riprodotto sulla revisione `f1ec558`: subito dopo `bootstrap_vm`, `(list verify-math fiber-step)` dà `(8 100)`.],
    cosa: [Il programma di verifica gira nell'ambiente globale e usa `setq`, e la documentazione di `bootstrap_vm` dice ora che le tre variabili restano legate. In caso di fallimento, l'errore restituito è `UncorrectFunctionDefinition`, che non dice nulla del problema.],
    correzione: [Eseguirlo in un ambiente figlio e restituire un errore che descriva il controllo fallito.],
  ),
  (
    id: "INT-20",
    gravita: "bassa",
    verificato: true,
    stato: "aperto",
    titolo: [`UncorrectFunctionDefinition` viene usato per errori che non c'entrano],
    breve: [Errore `UncorrectFunctionDefinition` fuorviante],
    dove: [`src/base/fibers.rs` (`primitive_resume`) e `src/base/atoms.rs` (`primitive_deref`, `primitive_reset`), per un lock avvelenato; `src/base/mod.rs` (`call_callable`), per un argomento che non è una funzione; `src/handshake.rs` (`bootstrap_vm`)],
    verifica: [Riprodotto sulla revisione `f1ec558`: `(funcall 5)` dà `(invalid-function "UncorrectFunctionDefinition")`; in rsedit lo stesso con `(with-current-buffer "*scratch*" (buffer-string))`.],
    cosa: [Lo stesso errore copre un lock avvelenato, un argomento che non è una funzione e un controllo d'avvio fallito. Da `039ca89` la sua documentazione elenca questi significati invece di separarli.],
    correzione: [Usare `WrongArgumentType` per gli argomenti, in `call_callable` (in rsedit `with-current-buffer` può anche controllare da sé il suo argomento), e un errore dedicato per i lock avvelenati e per il controllo d'avvio.],
  ),
  (
    id: "INT-21",
    gravita: "bassa",
    verificato: false,
    stato: "aperto",
    titolo: [Atomi senza aggiornamento atomico],
    breve: [Atomi senza compare-and-swap],
    dove: [`src/base/atoms.rs` (`make-atom`, `deref`, `reset`)],
    verifica: [Dalla lettura del codice.],
    cosa: [Esistono solo `deref` e `reset`, e ciascuno prende il lock per conto suo. Un incremento da due thread (`(reset a (+ 1 (deref a)))`) perde aggiornamenti, e nessuna primitiva permette di farlo in modo atomico. In più, gli esempi delle docstring di `deref` e `reset` creano l'atomo con `(atom 0)`, che è il predicato del tipo: il costruttore è `make-atom`.],
    correzione: [Una primitiva `swap` che applica una funzione sotto il lock, oppure `compare-and-set`; negli esempi, `(make-atom 0)`.],
  ),
  (
    id: "INT-22",
    gravita: "bassa",
    verificato: true,
    stato: "aperto",
    titolo: [Parametri chiamati `t` o `nil` sono accettati ma irraggiungibili],
    breve: [Parametri `t`/`nil` irraggiungibili],
    dove: [`src/utils.rs` (`parse_lambda_params`, `bind_lambda_args`), `src/eval.rs` (`eval_step_permitted`: i simboli che valutano a sé stessi sono risolti prima della ricerca)],
    verifica: [Riprodotto sulla revisione `f1ec558`: `((lambda (t) t) 5)` dà `t`.],
    cosa: [`(lambda (t) t)` viene definita senza errori, ma nel corpo `t` vale sempre `t`, perché i simboli che valutano a sé stessi non vengono cercati nell'ambiente. Lo stesso vale per `nil` e per le keyword. Accettano questi nomi anche `let`, `let*` (`parse_let_binding`) e `setq`.],
    correzione: [Rifiutare questi nomi in `parse_lambda_params`, in `parse_let_binding` e in `setq`.],
  ),
  (
    id: "INT-23",
    gravita: "bassa",
    verificato: true,
    stato: "aperto",
    titolo: [`function-doc` contiene un `unreachable!`],
    breve: [`unreachable!` in `function-doc`],
    dove: [`src/base/functions.rs` (`primitive_function_doc`)],
    verifica: [Riprodotto sulla revisione `f1ec558` con un host che lega un simbolo nello spazio delle funzioni: dopo `env.set_function("x", LispExp::symbol("car"))`, `(function-doc 'x)` va in panic.],
    cosa: [`primitive_function_doc` gestisce solo lambda e primitive e finisce con `_ => unreachable!()`. Dentro risp e in rsedit nessuno inserisce altro nello spazio delle funzioni; ma risp è ora un crate a sé, `Env::set_function` è pubblica e accetta qualunque `LispExp`, e `call_callable` segue già un simbolo legato lì come un alias. Un host che lo faccia manda `function-doc` in panic.],
    correzione: [`_ => Ok(LispExp::nil())`.],
  ),
  (
    id: "INT-24",
    gravita: "media",
    verificato: true,
    stato: "aperto",
    titolo: [`if` valuta solo la prima forma del ramo else e ignora le altre in silenzio],
    breve: [`if` ignora in silenzio le forme del ramo else dopo la prima],
    dove: [`src/eval.rs` (`eval_special_form_or_call_step`, il ramo `"if"`)],
    verifica: [Riprodotto sulla revisione `f1ec558`: `(setq x 0) (if nil 1 (setq x 1) (setq x 2)) x` dà 1; in Emacs dà 2.],
    cosa: [Il ramo `"if"` controlla che ci siano la condizione e il ramo then, valuta la condizione e poi continua, come chiamata in coda, con il ramo then oppure con `args[2]`. Gli argomenti da `args[3]` in poi non vengono mai valutati, e nessun errore lo dice. In Emacs Lisp il ramo else è un `progn` implicito: tutte le sue forme vengono valutate, e `if` restituisce il valore dell'ultima.],
    riproduzione: ```lisp
(setq x 0)
(if nil 1 (setq x 1) (setq x 2))   ; 1, e x vale 1; in Emacs 2 e 2
```,
    effetto: [Codice scritto come in Emacs perde in silenzio una parte del ramo else. È successo in rsedit: in `vim--pending-key`, nel modulo `vim`, un `if` aveva `(vim--reset)` come seconda forma del ramo else, e un conteggio davanti a `gg` o `ge` restava al comando dopo (`4gg` seguito da `dd` cancellava quattro righe). Il modulo è stato corretto in rsedit con `ef75b23`; l'interprete no.],
    correzione: [Valutare le forme del ramo else come quelle di `progn`, con `eval_body_step`, che dà anche il trattamento di `yield`; in alternativa rifiutare un `if` con più di tre argomenti con un errore, che almeno non fa sparire forme in silenzio. Un test per ciascun caso: il valore restituito e gli effetti di tutte le forme del ramo else.],
  ),
  // ------------------------------------------------------------------ DOC
  (
    id: "DOC-1",
    gravita: "bassa",
    verificato: false,
    stato: "aperto",
    titolo: [Il manuale dell'interprete descrive come attuali cinque difetti corretti],
    breve: [`architettura-interprete.typ` superato: lettore, carburante, cicli, percorsi],
    dove: [`doc/architettura-interprete.typ`],
    verifica: [Dalla lettura del documento sulla revisione `f1ec558`.],
    cosa: [Il manuale è entrato in `90e0eb4` e non è cambiato da allora. Descrive ancora il lettore ricorsivo, con `parse_list` e `UnexpectedDot` e senza il limite `TooDeep` (LET-1, LET-2, LET-3), il `ctx.begin_unwind()` di `unwind-protect` (INT-1) e `dotimes` e `dolist` senza carburante (INT-5), con i rimandi a quelle schede. Dice che i percorsi sono relativi a `core/src/lisp/`, mentre in risp sono relativi alla radice del repository. E la sua lista di controllo dice di togliere la scheda di un difetto corretto, mentre questo rapporto la segna come risolta.],
    correzione: [Riscrivere le sezioni del lettore, di `unwind-protect` e dei cicli sul codice di oggi, togliendo i rimandi alle schede risolte; correggere la radice dei percorsi e la lista di controllo.],
  ),
  (
    id: "DOC-2",
    gravita: "bassa",
    verificato: true,
    stato: "aperto",
    titolo: [Un file di test non viene mai compilato],
    breve: [`src/tests/reader_tests.rs` senza `mod`: non compila e non gira],
    dove: [`src/tests/reader_tests.rs`, `src/tests/mod.rs`],
    verifica: [Riprodotto sulla revisione `f1ec558`: dichiarato in `src/tests/mod.rs`, il file non compila, con quattro errori su `ParserError::UnexpectedDot`.],
    cosa: [Il file è arrivato da rsedit insieme all'interprete e contiene i test delle correzioni proposte nelle schede del lettore. Nessun `mod` lo dichiara, quindi `cargo test` non lo compila. Dichiarato, non compilerebbe: usa `ParserError::UnexpectedDot`, che `3b7cbac` ha sostituito con `UnexpectedToken(".")`. E alcune sue asserzioni descrivono come corretto il comportamento di difetti ancora aperti (LET-4, LET-5, LET-6).],
    correzione: [Dichiararlo, sostituire `UnexpectedDot`, e per i difetti ancora aperti segnare i test con `#[ignore]` e un `TODO(ID)`, oppure toglierli finché la scheda non si chiude.],
  ),
  (
    id: "DOC-3",
    gravita: "bassa",
    verificato: true,
    stato: "aperto",
    titolo: [`cargo clippy --all-targets` si ferma su un letterale dei test],
    breve: [clippy: `approx_constant` per `-3.14` in `parser_tests.rs`],
    dove: [`src/tests/parser_tests.rs` (`test_edge_negative_numbers`)],
    verifica: [Riprodotto sulla revisione `f1ec558`: `cargo clippy --all-targets` termina con «approximate value of `f{32, 64}::consts::PI` found», una lint di livello deny.],
    cosa: [Il letterale `-3.14` in un test del lettore viene scambiato da clippy per π. Il difetto viene dalla scheda DOC-4 del rapporto di rsedit, risolta per rsedit: il test è arrivato qui con l'interprete.],
    correzione: [Usare un altro letterale, per esempio `-2.5`.],
  ),
)

// ---------------------------------------------------------------------------
// Il documento
// ---------------------------------------------------------------------------

#let aperti = problemi.filter(p => p.stato == "aperto")
#let risolti = problemi.filter(p => p.stato == "risolto")
#let conta(g) = aperti.filter(p => p.gravita == g).len()
#let ordinati = aperti.sorted(key: p => (ordine-gravita.at(p.gravita), aree.position(a => a.sigla == area-di(p))))

#outline(depth: 2, indent: 1.2em)

= Che cosa contiene questo rapporto

Questo documento elenca i problemi trovati nel codice di risp, l'interprete Lisp di rsedit. È separato di proposito dal manuale, `architettura-interprete.typ`: il manuale descrive come il codice funziona e resta valido anche dopo una correzione; questo rapporto descrive dove il codice non fa ciò che dovrebbe. Quando un problema viene risolto, la sua scheda passa nell'appendice delle schede risolte, con il commit che lo ha risolto.

Le schede LET (il lettore) e INT (il valutatore e la libreria di base) vengono dall'analisi di rsedit della revisione `08a853d`, quando l'interprete era ancora la sua directory `core/src/lisp`. Dopo il passaggio a questo repository (`cee94f3`) sono state separate dal rapporto di rsedit, e hanno conservato i loro identificativi. Le schede DOC, sulla documentazione e i test, sono nate dalla verifica. Ogni identificativo si può citare in un commit o in una discussione; nel codice lo si cita solo in un `TODO(ID)`, e solo finché la scheda è aperta. Dove una scheda rimanda a una di rsedit lo dice nel testo, per esempio «CMD-3, nel rapporto di rsedit».

Le schede sono generate da un'unica tabella di dati all'inizio del sorgente: per aggiungerne una, o per segnarne una come risolta, basta modificare quella tabella, e il riepilogo si aggiorna da solo.

== Come leggere una scheda

Ogni scheda riporta:

- *Gravità*, secondo la scala qui sotto.
- *Dove*: i file e le funzioni, o i tipi, in cui sta il difetto, con i percorsi relativi alla radice del repository.
- *Verifica*: «verificato» se il comportamento è stato riprodotto eseguendo il codice (il testo dice come, e su quale revisione), «dal codice» se la conclusione viene dalla sola lettura.
- *Cosa succede*, il meccanismo del difetto; *Come riprodurlo*, quando serve; *Effetto*, ciò che vede chi usa l'interprete; *Correzione proposta*.

Tutte le schede dell'elenco sono aperte; le risolte sono nell'appendice.

#ref-table(
  columns: (2.2cm, 1fr),
  header: ([Gravità], [Significato]),
  [#etichetta-gravita("alta")], [Il processo che ospita l'interprete termina, o si blocca senza via d'uscita, per un'espressione che dovrebbe dare un errore Lisp.],
  [#etichetta-gravita("media")], [Un comportamento sbagliato o un rallentamento di secondi in un uso realistico, senza perdita di dati.],
  [#etichetta-gravita("bassa")], [Casi limite, incoerenze, codice morto, commenti e documentazione superati.],
)

== Come è stato condotto l'esame

Nell'analisi di rsedit `08a853d` l'interprete è stato letto per intero, ed esercitato con:

- un *fuzzer del lettore*, su tutte le stringhe fino a tre caratteri di un alfabeto di 28 simboli sintattici e su 300 000 stringhe casuali più lunghe;
- un *fuzzer delle forme speciali*, che ha valutato 181 322 forme speciali con sintassi malformata (liste di legami sbagliate, parametri impossibili, `yield` fuori posto) senza trovare alcun panic;
- il *fuzzer delle primitive* di rsedit, che chiamava anche le primitive di base con 807 combinazioni di argomenti anomali;
- piccoli programmi che valutano espressioni, per le riproduzioni.

Le misure di tempo e di memoria sono state prese con build di release su una macchina condivisa, quindi vanno lette come ordini di grandezza.

=== La verifica sulla revisione `f1ec558`

Ogni scheda è stata poi verificata sulla revisione `f1ec558`. Il codice che descrive è stato riletto, e la storia di git ha detto quale commit lo ha cambiato. Quasi tutte le riproduzioni sono state eseguite di nuovo, con un test temporaneo che valuta le espressioni della scheda in un contesto che consuma carburante; le misure che non sono state ripetute, la scheda lo dice. Il campo *Dove* nomina ora file e funzioni, non righe.

Delle 29 schede dell'interprete, cinque sono risolte: LET-1, LET-2, LET-3, INT-1 e INT-5. Una lo è in parte e resta aperta con il testo aggiornato a ciò che manca: INT-13. La verifica ne ha trovate quattro nuove: INT-24, DOC-1, DOC-2 e DOC-3. Tutte le correzioni sono già nella revisione `b2d000b`, quella che rsedit usa; dopo, `039ca89` ha cambiato solo commenti e `f1ec558` ha aggiunto una variante di `LispExp` senza usarla. Le schede aperte qui sono quindi aperte anche per rsedit.

== Riepilogo

#let n-alta = conta("alta")
#let n-media = conta("media")
#let n-bassa = conta("bassa")

#key[Sono aperti *#aperti.len() problemi*: *#n-alta di gravità alta*, #n-media di gravità media e #n-bassa di gravità bassa; #aperti.filter(p => p.verificato).len() sono stati riprodotti eseguendo il codice. Altri #risolti.len() sono stati risolti dopo l'analisi, e sono nell'appendice.]

Le correzioni del lettore (#link(<LET-1>)[LET-1], #link(<LET-2>)[LET-2], #link(<LET-3>)[LET-3]) e del carburante (#link(<INT-1>)[INT-1], #link(<INT-5>)[INT-5]) hanno tolto quasi tutti i modi in cui un'espressione scritta male bloccava o chiudeva l'editor che ospita l'interprete. Resta lo stack: una ricorsione profonda (#link(<INT-2>)[INT-2], l'unico problema di gravità alta) e una struttura annidata costruita da un programma (#link(<INT-4>)[INT-4]) lo esauriscono, e il processo termina. Per la memoria restano le chiusure che si tengono in vita a vicenda (#link(<INT-6>)[INT-6]) e `make-string` senza un limite (#link(<INT-13>)[INT-13]).

La tabella seguente elenca i problemi aperti, dal più grave; l'identificativo rimanda alla scheda.

#{
  set text(size: 8.5pt)
  show: codice-relativo
  ref-table(
    columns: (1.35cm, 1fr, 1.45cm, 1.6cm),
    header: ([ID], [Problema], [Gravità], [Verifica]),
    ..ordinati.map(p => (
      link(label(p.id))[#p.id],
      p.breve,
      etichetta-gravita(p.gravita),
      etichetta-verifica(p.verificato),
    )).flatten(),
  )
}

#{
  let righe = aree.map(a => {
    let qui = aperti.filter(p => area-di(p) == a.sigla)
    (
      [#a.sigla],
      a.nome,
      [#qui.filter(p => p.gravita == "alta").len()],
      [#qui.filter(p => p.gravita == "media").len()],
      [#qui.filter(p => p.gravita == "bassa").len()],
      [#risolti.filter(p => area-di(p) == a.sigla).len()],
    )
  })
  figure(
    ref-table(
      columns: (1.2cm, 1fr, 1.2cm, 1.2cm, 1.2cm, 1.4cm),
      header: ([Area], [Descrizione], [Alta], [Media], [Bassa], [Risolte]),
      ..righe.flatten(),
    ),
    caption: [I problemi aperti per area e gravità, e quelli risolti],
  )
}

// Le schede aperte, area per area.
#for a in aree {
  let qui = aperti.filter(p => area-di(p) == a.sigla)
  heading(level: 1)[#a.nome]
  if qui.len() == 0 [Nessuna scheda aperta: quelle dell'area sono tra le risolte.]
  for p in qui {
    scheda(p)
  }
}

= Appendici

== Le schede risolte

Le schede risolte dopo l'analisi, con il commit che le ha risolte. Il testo completo di ciascuna sta nel rapporto di rsedit, `doc/rapporto-problemi.typ`, fino alla revisione `3d846f1`; che cosa è cambiato, e perché, nel messaggio del commit.

#{
  set text(size: 8.5pt)
  show: codice-relativo
  let ordine = risolti.sorted(key: p => (aree.position(a => a.sigla == area-di(p)), int(p.id.split("-").at(1))))
  ref-table(
    columns: (1.35cm, 1fr, 1.45cm, 1.7cm),
    header: ([ID], [Problema], [Gravità], [Risolto in]),
    ..ordine.map(p => (
      [#metadata(p.id)#label(p.id)#p.id],
      p.titolo,
      etichetta-gravita(p.gravita),
      raw(p.commit),
    )).flatten(),
  )
}

== Differenze da Emacs che non sono difetti

Alcuni comportamenti differiscono da Emacs per scelta, o comunque senza produrre risultati sbagliati. Sono elencati qui perché chi conosce Emacs non li scambi per errori.

- `%` ha la stessa semantica di `mod`: il segno del risultato segue il divisore. In Emacs `%` è il resto e segue il dividendo: `(% -7 2)` dà 1 qui e −1 in Emacs.
- `out-of-fuel` non si intercetta con `condition-case`. È voluto: un ciclo infinito non deve potersi proteggere dal meccanismo che lo ferma. I gestori di `unwind-protect` girano comunque.
- Il lettore rifiuta un'espressione annidata più di 256 livelli, con l'errore `TooDeep`: è il limite che ha risolto #link(<LET-3>)[LET-3]. Emacs legge strutture più profonde.

== Parti esaminate e trovate solide

Nell'analisi tutte le forme speciali dell'interprete sono state esercitate con sintassi malformata (181 322 forme valutate dal fuzzer) senza trovare un panic.
