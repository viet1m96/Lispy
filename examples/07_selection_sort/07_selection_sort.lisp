; Selection sort

(begin
  (setq codes :array (array 4))

  (array-set codes 0 100)
  (array-set codes 1 98)
  (array-set codes 2 99)
  (array-set codes 3 97)

  (setq n :int (array-size codes))
  (setq i :int 0)

  (loop while (< i n) do
    (setq best :int i)
    (setq j :int (+ i 1))

    (loop while (< j n) do
      (if (< (array-get codes j) (array-get codes best))
          (setq best :int j)
          0)

      (setq j :int (+ j 1))
    finally 0)

    (if (!= best i)
        (let ((tmp :int (array-get codes i)))
          (begin
            (array-set codes i (array-get codes best))
            (array-set codes best tmp)))
        0)

    (setq i :int (+ i 1))
  finally 0)

  (print (array-get codes 0))
  (print-str " ")
  (print (array-get codes 1))
  (print-str " ")
  (print (array-get codes 2))
  (print-str " ")
  (print (array-get codes 3))
  (print-str "\n")

  (halt))
