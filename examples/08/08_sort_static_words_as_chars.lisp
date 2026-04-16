; Selection sort to sort symbols in a string

(begin
  (setq s "dbca")
  (setq n (strlen s))
  (setq i 0)

  (loop while (< i n) do
    (setq best i)
    (setq j (+ i 1))
    (loop while (< j n) do
      (if (< (strget s j) (strget s best))
          (setq best j)
          0)
      (setq j (+ j 1))
    finally 0)

    (if (!= best i)
        (let ((tmp (strget s i)))
          (begin
            (strset s i (strget s best))
            (strset s best tmp)))
        0)

    (setq i (+ i 1))
  finally 0)

  (print-str s)
  (halt))
