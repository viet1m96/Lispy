; Pascal string operations

(begin
  (setq s :string "Hello")
  (print-str s)
  (print-str "\n")

  (print (strlen s))
  (print-str "\n")

  ; ASCII 89 = 'Y', so Hello becomes Yello.
  (strset s 0 89)
  (print-str s)
  (halt))
